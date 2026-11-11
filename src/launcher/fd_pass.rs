//! Unix domain socket ancillary data (`SCM_RIGHTS`) file descriptor passing.

use std::{
    io::{IoSlice, IoSliceMut},
    os::fd::{FromRawFd, OwnedFd, RawFd},
};

use nix::sys::socket::{ControlMessage, ControlMessageOwned, MsgFlags, recvmsg, sendmsg};

/// Sends 1 dummy byte with `SCM_RIGHTS` control message containing `fd_to_send`
/// using `nix::sys::socket::sendmsg`.
pub fn send_fd(sock: RawFd, fd_to_send: RawFd) -> nix::Result<()> {
    let dummy = [0u8; 1];
    let iov = [IoSlice::new(&dummy)];
    let fds = [fd_to_send];
    let cmsg = ControlMessage::ScmRights(&fds);
    sendmsg::<()>(sock, &iov, &[cmsg], MsgFlags::empty(), None)?;
    Ok(())
}

/// Receives 1 dummy byte and extracts the received `RawFd` from `SCM_RIGHTS`
/// control message using `nix::sys::socket::recvmsg`, returning an `OwnedFd`.
pub fn recv_fd(sock: RawFd) -> nix::Result<OwnedFd> {
    let mut dummy = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut dummy)];
    let mut cmsg_buf = nix::cmsg_space!(RawFd);
    let msg = recvmsg::<()>(
        sock,
        &mut iov,
        Some(&mut cmsg_buf),
        MsgFlags::MSG_CMSG_CLOEXEC,
    )?;

    if msg.bytes == 0 {
        return Err(nix::Error::ECONNRESET);
    }

    for cmsg in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(fds) = cmsg {
            let mut iter = fds.into_iter();
            if let Some(fd) = iter.next() {
                // Ensure any unexpected extra file descriptors are cleaned up.
                for extra in iter {
                    let _ = nix::unistd::close(extra);
                }
                // SAFETY: `fd` is a valid file descriptor allocated by the kernel
                // for this process upon receiving the SCM_RIGHTS message.
                return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
    }

    Err(nix::Error::EBADMSG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::socket::{AddressFamily, SockFlag, SockType, socketpair};
    use std::os::fd::AsRawFd;

    #[test]
    fn test_send_and_recv_fd() {
        let (p1, p2) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .expect("socketpair should succeed");

        let (read_pipe, write_pipe) = nix::unistd::pipe().expect("pipe should succeed");

        send_fd(p1.as_raw_fd(), read_pipe.as_raw_fd()).expect("send_fd should succeed");
        let received_fd = recv_fd(p2.as_raw_fd()).expect("recv_fd should succeed");

        assert!(received_fd.as_raw_fd() >= 0);

        // Verify data transmission over the transferred file descriptor
        let payload = b"statis deterministic runtime";
        nix::unistd::write(&write_pipe, payload).expect("pipe write should succeed");

        let mut buf = vec![0u8; payload.len()];
        let n =
            nix::unistd::read(received_fd.as_raw_fd(), &mut buf).expect("pipe read should succeed");
        assert_eq!(&buf[..n], payload);
    }

    #[test]
    fn test_recv_from_closed_socket() {
        let (p1, p2) = socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::SOCK_CLOEXEC,
        )
        .expect("socketpair should succeed");

        // Close peer socket immediately
        drop(p1);

        let err = recv_fd(p2.as_raw_fd()).unwrap_err();
        assert!(err == nix::Error::ECONNRESET || err == nix::Error::EBADMSG);
    }
}
