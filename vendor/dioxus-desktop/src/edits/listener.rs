// Local patch: stop the edits listener before the desktop event loop exits.
// See ../../PATCHES.md for provenance and the scope of this change.

use std::io;
use std::net::{Shutdown as SocketShutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// The listener thread is shared by every EditWebsocket clone. The thread only
/// owns Shutdown, so dropping the last owner can stop and join it.
pub(super) struct ServerThread {
    shutdown: Arc<Shutdown>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl ServerThread {
    pub(super) fn spawn(
        address: SocketAddr,
        run: impl FnOnce(Arc<Shutdown>) + Send + 'static,
    ) -> Self {
        let shutdown = Arc::new(Shutdown {
            state: Mutex::new(State {
                stopped: false,
                address,
                handshake: None,
            }),
            changed: Condvar::new(),
        });
        let thread_shutdown = shutdown.clone();
        Self {
            shutdown,
            thread: Mutex::new(Some(thread::spawn(move || run(thread_shutdown)))),
        }
    }

    pub(super) fn shutdown(&self) {
        // This lock is never used by the listener. It serializes callers while
        // allowing the listener to acquire its state lock during join.
        let mut guard = self.thread.lock().unwrap();
        let result = if let Some(thread) = guard.take() {
            self.shutdown.stop();
            thread.join()
        } else {
            Ok(())
        };
        drop(guard);
        if let Err(panic) = result {
            // Preserve genuine listener panics instead of hiding them. Release
            // our own lock first so unwinding can safely run Drop.
            std::panic::resume_unwind(panic);
        }
    }
}

impl Drop for ServerThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct State {
    stopped: bool,
    address: SocketAddr,
    handshake: Option<TcpStream>,
}

pub(super) struct Shutdown {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shutdown {
    pub(super) fn is_stopped(&self) -> bool {
        self.state.lock().unwrap().stopped
    }

    /// Publish a replacement listener only while the application is running.
    pub(super) fn listen_on(&self, address: SocketAddr) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.stopped {
            return false;
        }
        state.address = address;
        true
    }

    /// Register the handshake socket before any blocking reads. Stop can close
    /// its clone even if a local client never sends an HTTP upgrade request.
    pub(super) fn handshake(&self, stream: &TcpStream) -> io::Result<Option<Handshake<'_>>> {
        let mut state = self.state.lock().unwrap();
        if state.stopped {
            return Ok(None);
        }
        // On Windows, shutdown on a duplicate socket does not cancel a recv
        // already pending on the original handle. Bound this initial HTTP/auth
        // exchange so an idle local client cannot prevent application exit.
        // The timeouts are cleared before the normal edits worker takes over.
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        state.handshake = Some(stream.try_clone()?);
        Ok(Some(Handshake { shutdown: self }))
    }

    pub(super) fn wait_to_retry(&self, delay: Duration) -> bool {
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, delay, |state| !state.stopped)
            .unwrap();
        !state.stopped
    }

    fn stop(&self) {
        let address = {
            let mut state = self.state.lock().unwrap();
            state.stopped = true;
            if let Some(stream) = &state.handshake {
                _ = stream.shutdown(SocketShutdown::Both);
            }
            state.address
        };
        self.changed.notify_all();
        // Blocking accept has no idle polling. A single local connection wakes
        // it; the stopped check rejects the wakeup before the handshake starts.
        // No state lock is held during connect or while joining the thread.
        _ = TcpStream::connect_timeout(&address, Duration::from_millis(500));
    }
}

pub(super) struct Handshake<'a> {
    shutdown: &'a Shutdown,
}

impl Drop for Handshake<'_> {
    fn drop(&mut self) {
        self.shutdown.state.lock().unwrap().handshake = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;

    const TEST_TIMEOUT: Duration = Duration::from_secs(3);

    fn server() -> (TcpListener, SocketAddr) {
        let server = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = server.local_addr().unwrap();
        (server, address)
    }

    #[test]
    fn shutdown_wakes_blocking_accept_and_releases_the_port() {
        let (server, address) = server();
        let (entered_tx, entered_rx) = mpsc::channel();
        let listener = ServerThread::spawn(address, move |shutdown| {
            entered_tx.send(()).unwrap();
            let (stream, _) = server.accept().unwrap();
            assert!(shutdown.handshake(&stream).unwrap().is_none());
        });
        entered_rx.recv_timeout(TEST_TIMEOUT).unwrap();
        listener.shutdown();
        listener.shutdown();
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    fn shutdown_interrupts_an_idle_handshake() {
        let (server, address) = server();
        let (entered_tx, entered_rx) = mpsc::channel();
        let listener = ServerThread::spawn(address, move |shutdown| {
            let (mut stream, _) = server.accept().unwrap();
            let _handshake = shutdown.handshake(&stream).unwrap().unwrap();
            entered_tx.send(()).unwrap();
            // The client stays connected without sending an upgrade request.
            let result = stream.read(&mut [0]);
            assert!(matches!(result, Ok(0) | Err(_)));
            assert!(shutdown.is_stopped());
        });
        let _client = TcpStream::connect(address).unwrap();
        entered_rx.recv_timeout(TEST_TIMEOUT).unwrap();
        listener.shutdown();
        assert!(TcpStream::connect_timeout(&address, TEST_TIMEOUT).is_err());
    }

    #[test]
    fn shutdown_interrupts_reconnect_backoff() {
        let (server, address) = server();
        let (entered_tx, entered_rx) = mpsc::channel();
        let listener = ServerThread::spawn(address, move |shutdown| {
            drop(server);
            entered_tx.send(()).unwrap();
            assert!(!shutdown.wait_to_retry(Duration::from_secs(30)));
            assert!(!shutdown.listen_on(address));
        });
        entered_rx.recv_timeout(TEST_TIMEOUT).unwrap();
        listener.shutdown();
    }

    #[test]
    fn shutdown_uses_the_replacement_listener_address() {
        let (first, first_address) = server();
        let (replacement, replacement_address) = server();
        let (entered_tx, entered_rx) = mpsc::channel();
        let listener = ServerThread::spawn(first_address, move |shutdown| {
            drop(first);
            assert!(shutdown.listen_on(replacement_address));
            entered_tx.send(()).unwrap();
            let (stream, _) = replacement.accept().unwrap();
            assert!(shutdown.handshake(&stream).unwrap().is_none());
        });
        entered_rx.recv_timeout(TEST_TIMEOUT).unwrap();
        listener.shutdown();
        assert!(TcpListener::bind(replacement_address).is_ok());
    }

    #[test]
    fn last_owner_drop_stops_the_listener() {
        let (server, address) = server();
        let listener = Arc::new(ServerThread::spawn(address, move |shutdown| {
            let (stream, _) = server.accept().unwrap();
            assert!(shutdown.handshake(&stream).unwrap().is_none());
        }));
        let last_owner = listener.clone();
        drop(listener);
        assert!(!last_owner.shutdown.is_stopped());
        drop(last_owner);
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    fn simultaneous_shutdown_callers_wait_for_the_same_thread() {
        let (server, address) = server();
        let listener = Arc::new(ServerThread::spawn(address, move |shutdown| {
            let (stream, _) = server.accept().unwrap();
            assert!(shutdown.handshake(&stream).unwrap().is_none());
        }));
        let other = listener.clone();
        let concurrent = thread::spawn(move || other.shutdown());
        listener.shutdown();
        concurrent.join().unwrap();
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    #[should_panic(expected = "listener test panic")]
    fn listener_panics_are_propagated() {
        let (_server, address) = server();
        let listener = ServerThread::spawn(address, |_| panic!("listener test panic"));
        listener.shutdown();
    }
}
