//! systemd socket activation adapters for the Direct HTTPS listeners.
//!
//! In Direct subscription mode systemd owns the public TCP 80/443 listeners
//! through `sbctl-http.socket` and passes the already-bound sockets to the
//! non-root sbctl service. The service takes the descriptors via `LISTEN_PID`
//! and `LISTEN_FDS` and routes each one by its local port: 80 serves the ACME
//! HTTP-01 challenge, 443 serves the TLS subscription. (ADR-0011)

use std::env;
use std::io;
use std::net::TcpListener;
use std::os::fd::FromRawFd;

use thiserror::Error;

/// The first descriptor systemd passes, per the `sd_listen_fds` protocol.
const FIRST_FD: i32 = 3;

#[derive(Debug, Error)]
pub enum SocketActivationError {
    #[error("systemd did not provide socket activation; Direct HTTPS requires sbctl-http.socket")]
    NotSocketActivated,
    #[error("LISTEN_PID does not match this process")]
    PidMismatch,
    #[error("LISTEN_FDS is not a valid descriptor count")]
    InvalidFds,
    #[error("could not read listener descriptor {0}: {1}")]
    Listener(usize, io::Error),
}

/// Validates the systemd `LISTEN_PID`/`LISTEN_FDS` variables and returns the
/// number of file descriptors that were passed. `LISTEN_PID` must equal the
/// current process id so a stale variable from a parent cannot leak
/// descriptors into the daemon.
pub fn parse_listen_fds(pid: &str, fds: &str) -> Result<usize, SocketActivationError> {
    let expected: u32 = pid
        .parse()
        .map_err(|_| SocketActivationError::PidMismatch)?;
    if expected != std::process::id() {
        return Err(SocketActivationError::PidMismatch);
    }
    let count: u32 = fds.parse().map_err(|_| SocketActivationError::InvalidFds)?;
    if count == 0 || count > u16::MAX as u32 {
        return Err(SocketActivationError::InvalidFds);
    }
    Ok(count as usize)
}

/// Takes the listeners passed by systemd socket activation, returning each one
/// with its bound local port. Descriptors begin at `FIRST_FD` and are closed
/// when the returned listeners are dropped. Only call after a real socket
/// activation handoff; the environment variables are the handoff marker.
pub fn receive_listeners() -> Result<Vec<(u16, TcpListener)>, SocketActivationError> {
    let pid = env::var("LISTEN_PID").map_err(|_| SocketActivationError::NotSocketActivated)?;
    let fds = env::var("LISTEN_FDS").map_err(|_| SocketActivationError::NotSocketActivated)?;
    let count = parse_listen_fds(&pid, &fds)?;
    let mut listeners = Vec::with_capacity(count);
    for index in 0..count {
        let listener = unsafe { TcpListener::from_raw_fd(FIRST_FD + index as i32) };
        let port = listener
            .local_addr()
            .map_err(|error| SocketActivationError::Listener(index, error))?
            .port();
        listeners.push((port, listener));
    }
    Ok(listeners)
}

/// The role a Direct HTTPS listener plays, decided solely by its local port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectListenerRole {
    /// TCP 80: ACME HTTP-01 challenge webroot.
    Acme,
    /// TCP 443: TLS subscription endpoint.
    Tls,
}

/// Maps a local port to its Direct HTTPS role. Any other port is an unexpected
/// or unowned listener and must not be served.
pub fn direct_listener_role(port: u16) -> Option<DirectListenerRole> {
    match port {
        80 => Some(DirectListenerRole::Acme),
        443 => Some(DirectListenerRole::Tls),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectListenerRole, direct_listener_role, parse_listen_fds};

    #[test]
    fn parse_listen_fds_returns_the_descriptor_count_for_this_process() {
        let pid = std::process::id().to_string();
        assert_eq!(
            parse_listen_fds(&pid, "2").expect("two sockets are valid"),
            2
        );
        assert_eq!(parse_listen_fds(&pid, "1").expect("one socket is valid"), 1);
    }

    #[test]
    fn parse_listen_fds_rejects_a_stale_pid() {
        assert!(matches!(
            parse_listen_fds("1", "2"),
            Err(super::SocketActivationError::PidMismatch)
        ));
        assert!(matches!(
            parse_listen_fds("not-a-pid", "2"),
            Err(super::SocketActivationError::PidMismatch)
        ));
    }

    #[test]
    fn parse_listen_fds_rejects_an_invalid_descriptor_count() {
        let pid = std::process::id().to_string();
        assert!(matches!(
            parse_listen_fds(&pid, "0"),
            Err(super::SocketActivationError::InvalidFds)
        ));
        assert!(matches!(
            parse_listen_fds(&pid, "not-a-count"),
            Err(super::SocketActivationError::InvalidFds)
        ));
        assert!(matches!(
            parse_listen_fds(&pid, "65536"),
            Err(super::SocketActivationError::InvalidFds)
        ));
    }

    #[test]
    fn receive_listeners_requires_a_socket_activation_handoff() {
        assert!(matches!(
            super::receive_listeners(),
            Err(super::SocketActivationError::NotSocketActivated)
        ));
    }

    #[test]
    fn direct_listener_role_routes_eighty_to_acme_and_four_forty_three_to_tls() {
        assert_eq!(direct_listener_role(80), Some(DirectListenerRole::Acme));
        assert_eq!(direct_listener_role(443), Some(DirectListenerRole::Tls));
        assert_eq!(direct_listener_role(0), None);
        assert_eq!(direct_listener_role(2080), None);
        assert_eq!(direct_listener_role(65535), None);
    }
}
