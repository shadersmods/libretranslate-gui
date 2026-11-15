# Local desktop listener shutdown patch

This is the crates.io source of `dioxus-desktop 0.7.10` (MIT OR Apache-2.0).
Its package provenance records Dioxus commit
`57d6794ad60b949e5bd8aa282f6f8c3dc97a365e`, `packages/desktop`.
The original normalized manifest and `Cargo.toml.orig` are preserved.
Registry bookkeeping, the package lockfile, and editor settings are omitted.
`LICENSE-MIT` and `LICENSE-APACHE` contain the upstream license terms.

Upstream: <https://github.com/DioxusLabs/dioxus/tree/57d6794ad60b949e5bd8aa282f6f8c3dc97a365e/packages/desktop>.

## Changes

- `src/config.rs` and `src/app.rs` add `WindowStaysOpen`, allowing the app to
  acknowledge pending saves before closing. Native close requests leave the
  window visible while the app handles them; successful saves switch back to
  `WindowCloses`. Existing upstream close modes retain their behavior.
- `src/edits/listener.rs` adds an owned listener thread with explicit shutdown
  and a last-owner drop fallback. Shutdown wakes blocking accept with one
  loopback connection and shuts down an in-progress handshake socket. There is no
  periodic accept polling, extra Winsock reference, or panic filtering. The
  initial HTTP/authentication exchange has a one-second read/write timeout:
  Windows does not cancel an already pending read when a duplicate socket is
  shut down. Both timeouts are cleared before normal edit traffic. Only an
  unresponsive handshake can add up to one second to listener shutdown.
- `src/edits.rs` registers blocking handshakes for cancellation, handles an
  interrupted authentication send as a connection error, and makes listener
  reconnect fallible. Failed reconnects wait for one second on a cancellation
  condition variable. The initial bind still fails loudly. Location changes
  are published before notifying the webview.
- `src/app.rs` stops and joins the listener on `LoopDestroyed`, before Tao exits
  the process and Rust releases Winsock. It also releases pending connection
  senders; the existing established-websocket worker protocol is unchanged.

The previous detached listener retried every accept error by binding again.
Normal Windows exit could leave it rebinding after Winsock cleanup and panic
with WSA10093. Explicit lifecycle shutdown prevents this race without bypassing
the normal application close path. Other listener panics still propagate.

## Validation

The listener module has standard-library-only tests for blocking accept,
an idle handshake, cancellation during reconnect backoff, replacement listener
addresses, last-owner drop, and genuine listener panic propagation. The
containing edits module also exercises the authenticated UI edit protocol.

Remove this patch after upgrading to an upstream release with equivalent
listener lifecycle handling, then repeat normal close and updater restart tests.
