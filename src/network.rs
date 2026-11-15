use std::{
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortState {
    Available,
    InUse,
    Unavailable,
}

pub fn probe_port(port: u16) -> PortState {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
        return PortState::InUse;
    }
    match TcpListener::bind(address) {
        Ok(_) => PortState::Available,
        Err(_) => PortState::Unavailable,
    }
}

pub fn find_free_port(start: u16, assigned: &[u16]) -> Option<u16> {
    (start.max(1024)..=u16::MAX)
        .chain(1024..start.max(1024))
        .find(|port| !assigned.contains(port) && probe_port(*port) == PortState::Available)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_live_listener_and_skips_assigned_ports() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let used = listener.local_addr().unwrap().port();
        assert_eq!(probe_port(used), PortState::InUse);
        let chosen = find_free_port(used, &[used.saturating_add(1)]).unwrap();
        assert_ne!(chosen, used);
        assert_ne!(chosen, used.saturating_add(1));
        assert_eq!(probe_port(chosen), PortState::Available);
    }
}
