//! Descriptor validation and non-consuming transport liveness. The supervised
//! PipeWire and EI consumers share only their own portal-granted sockets.
use std::{
    io,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    time::Duration,
};

pub(crate) struct Transports {
    pipewire: OwnedFd,
    eis: OwnedFd,
}

impl Transports {
    pub(crate) fn input_fd(&self) -> Result<OwnedFd, String> {
        self.eis
            .try_clone()
            .map_err(|e| format!("duplicate granted EIS fd: {e}"))
    }
    // An internal duplicate lets the supervisor monitor socket shutdown while
    // PipeWire owns its protocol fd. Both stay inside the same run lifetime;
    // teardown joins the consumer before dropping the monitoring descriptor.
    pub(crate) fn capture_fd(&self) -> Result<OwnedFd, String> {
        self.pipewire
            .try_clone()
            .map_err(|e| format!("duplicate granted PipeWire fd: {e}"))
    }
    pub(crate) fn new(pipewire: OwnedFd, eis: OwnedFd) -> Result<Self, String> {
        let result = Self {
            pipewire: connected_socket(pipewire, "PipeWire")?,
            eis: connected_socket(eis, "EIS")?,
        };
        if let Some(name) = result.disconnected() {
            return Err(format!("{name} transport already disconnected"));
        }
        Ok(result)
    }

    pub(crate) async fn until_disconnected(&self) -> &'static str {
        let mut check = tokio::time::interval(Duration::from_millis(25));
        check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            check.tick().await;
            if let Some(name) = self.disconnected() {
                return name;
            }
        }
    }

    fn disconnected(&self) -> Option<&'static str> {
        for (fd, name) in [(&self.pipewire, "PipeWire"), (&self.eis, "EIS")] {
            let mut pollfd = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLRDHUP,
                revents: 0,
            };
            // Owned fd stays alive during this zero-timeout, non-consuming poll.
            let result = unsafe { libc::poll(&mut pollfd, 1, 0) };
            if result < 0 {
                // EINTR is not evidence of disconnect. Other uncertainty revokes.
                if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    return Some(name);
                }
            } else if pollfd.revents
                & (libc::POLLHUP | libc::POLLRDHUP | libc::POLLERR | libc::POLLNVAL)
                != 0
            {
                return Some(name);
            }
        }
        None
    }
}

fn connected_socket(fd: OwnedFd, name: &str) -> Result<OwnedFd, String> {
    let raw = fd.as_raw_fd();
    let mut kind: libc::c_int = 0;
    let mut len = std::mem::size_of_val(&kind) as libc::socklen_t;
    // Pointer/length refer to the initialized local c_int; fd is owned here.
    let result = unsafe {
        libc::getsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    if result != 0 || kind != libc::SOCK_STREAM || len as usize != std::mem::size_of_val(&kind) {
        return Err(format!("{name} grant is not a stream socket"));
    }
    let mut domain: libc::c_int = 0;
    let mut domain_len = std::mem::size_of_val(&domain) as libc::socklen_t;
    // Linux portal transports must be local AF_UNIX, not an arbitrary TCP fd.
    let result = unsafe {
        libc::getsockopt(
            raw,
            libc::SOL_SOCKET,
            libc::SO_DOMAIN,
            (&mut domain as *mut libc::c_int).cast(),
            &mut domain_len,
        )
    };
    if result != 0
        || domain != libc::AF_UNIX
        || domain_len as usize != std::mem::size_of_val(&domain)
    {
        return Err(format!("{name} grant is not a Unix socket"));
    }
    let stream = UnixStream::from(fd);
    stream
        .peer_addr()
        .map_err(|_| format!("{name} grant is not a connected Unix socket"))?;
    // Defense in depth for direct callers. D-Bus must already receive the
    // original SCM_RIGHTS fd with atomic CLOEXEC; protecting this deserialized
    // duplicate alone cannot prevent concurrent exec from inheriting the original.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(format!("could not make {name} descriptor close-on-exec"));
    }
    Ok(stream.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn liveness_poll_does_not_consume_protocol_bytes_and_sets_close_on_exec() {
        let (pw, mut pw_peer) = UnixStream::pair().unwrap();
        let (ei, mut ei_peer) = UnixStream::pair().unwrap();
        assert_eq!(unsafe { libc::fcntl(pw.as_raw_fd(), libc::F_SETFD, 0) }, 0);
        let transports = Transports::new(pw.into(), ei.into()).unwrap();
        assert_ne!(
            unsafe { libc::fcntl(transports.pipewire.as_raw_fd(), libc::F_GETFD) }
                & libc::FD_CLOEXEC,
            0
        );
        pw_peer.write_all(b"pw-frame").unwrap();
        ei_peer.write_all(b"ei-event").unwrap();
        for _ in 0..8 {
            assert_eq!(transports.disconnected(), None);
        }
        for fd in [&transports.pipewire, &transports.eis] {
            let mut reader = UnixStream::from(fd.try_clone().unwrap());
            reader
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let mut data = [0u8; 8];
            reader.read_exact(&mut data).unwrap();
            assert!(data == *b"pw-frame" || data == *b"ei-event");
        }
        drop(ei_peer);
        assert_eq!(transports.disconnected(), Some("EIS"));
    }

    #[test]
    fn tcp_datagram_and_already_disconnected_transports_are_rejected() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server, _) = listener.accept().unwrap();
        assert!(connected_socket(client.into(), "test")
            .unwrap_err()
            .contains("not a Unix socket"));
        let (datagram, _peer) = std::os::unix::net::UnixDatagram::pair().unwrap();
        assert!(connected_socket(datagram.into(), "test")
            .unwrap_err()
            .contains("not a stream socket"));
        let (pw, pw_peer) = UnixStream::pair().unwrap();
        let (ei, _ei_peer) = UnixStream::pair().unwrap();
        drop(pw_peer);
        assert!(
            matches!(Transports::new(pw.into(), ei.into()), Err(message) if message.contains("already disconnected"))
        );
    }
}
