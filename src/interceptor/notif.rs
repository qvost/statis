//! Linux seccomp user notification ABI and ioctl wrappers.

use std::{io, mem::size_of, os::fd::RawFd};

use anyhow::Context;

/// Syscall metadata supplied by the kernel to a seccomp filter.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct seccomp_data {
    pub nr: i32,
    pub arch: u32,
    pub instruction_pointer: u64,
    pub args: [u64; 6],
}

/// A pending syscall received from a seccomp listener.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct seccomp_notif {
    pub id: u64,
    pub pid: u32,
    pub flags: u32,
    pub data: seccomp_data,
}

/// A response to a pending syscall. `error` is zero or a negative errno.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct seccomp_notif_resp {
    pub id: u64,
    pub val: i64,
    pub error: i32,
    pub flags: u32,
}

pub const SECCOMP_IOCTL_NOTIF_RECV: libc::c_ulong =
    nix::request_code_readwrite!(b'!', 0, size_of::<seccomp_notif>()) as libc::c_ulong;
pub const SECCOMP_IOCTL_NOTIF_SEND: libc::c_ulong =
    nix::request_code_readwrite!(b'!', 1, size_of::<seccomp_notif_resp>()) as libc::c_ulong;
pub const SECCOMP_IOCTL_NOTIF_ID_VALID: libc::c_ulong =
    nix::request_code_write!(b'!', 2, size_of::<u64>()) as libc::c_ulong;

pub const SECCOMP_USER_NOTIF_FLAG_CONTINUE: u32 = 1;

/// Receive a notification, blocking according to the listener's configuration.
pub fn recv_notification(fd: RawFd) -> anyhow::Result<seccomp_notif> {
    // RECV requires its input buffer to be zeroed.
    let mut notification = seccomp_notif::default();
    // SAFETY: the ioctl receives writable storage matching its encoded ABI size.
    let result = unsafe { libc::ioctl(fd, SECCOMP_IOCTL_NOTIF_RECV, &mut notification) };
    if result < 0 {
        return Err(io::Error::last_os_error()).context("receiving seccomp notification");
    }
    Ok(notification)
}

/// Send a response for the notification identified by `resp.id`.
pub fn send_response(fd: RawFd, resp: &seccomp_notif_resp) -> anyhow::Result<()> {
    // SEND is encoded as read/write, so provide a writable local copy.
    let mut response = *resp;
    // SAFETY: response is valid writable storage for the ioctl's ABI type.
    let result = unsafe { libc::ioctl(fd, SECCOMP_IOCTL_NOTIF_SEND, &mut response) };
    if result < 0 {
        return Err(io::Error::last_os_error()).context("sending seccomp notification response");
    }
    Ok(())
}

/// Check whether `id` is still pending; returns false on any ioctl failure.
///
/// A successful check does not prevent the notification from expiring afterward.
pub fn is_id_valid(fd: RawFd, id: u64) -> bool {
    // SAFETY: id points to readable storage matching the ioctl's u64 input.
    unsafe { libc::ioctl(fd, SECCOMP_IOCTL_NOTIF_ID_VALID, &id) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};

    #[test]
    fn abi_matches_linux_x86_64() {
        assert_eq!(size_of::<seccomp_data>(), 64);
        assert_eq!(align_of::<seccomp_data>(), 8);
        assert_eq!(offset_of!(seccomp_data, nr), 0);
        assert_eq!(offset_of!(seccomp_data, arch), 4);
        assert_eq!(offset_of!(seccomp_data, instruction_pointer), 8);
        assert_eq!(offset_of!(seccomp_data, args), 16);
        assert_eq!(size_of::<seccomp_notif>(), 80);
        assert_eq!(offset_of!(seccomp_notif, id), 0);
        assert_eq!(offset_of!(seccomp_notif, pid), 8);
        assert_eq!(offset_of!(seccomp_notif, flags), 12);
        assert_eq!(offset_of!(seccomp_notif, data), 16);
        assert_eq!(size_of::<seccomp_notif_resp>(), 24);
        assert_eq!(offset_of!(seccomp_notif_resp, id), 0);
        assert_eq!(offset_of!(seccomp_notif_resp, val), 8);
        assert_eq!(offset_of!(seccomp_notif_resp, error), 16);
        assert_eq!(offset_of!(seccomp_notif_resp, flags), 20);
        assert_eq!(SECCOMP_IOCTL_NOTIF_RECV, 0xc050_2100);
        assert_eq!(SECCOMP_IOCTL_NOTIF_SEND, 0xc018_2101);
        assert_eq!(SECCOMP_IOCTL_NOTIF_ID_VALID, 0x4008_2102);
    }

    #[test]
    fn invalid_descriptor_reports_errors() {
        let error = recv_notification(-1).unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().raw_os_error(),
            Some(libc::EBADF)
        );
        let error = send_response(-1, &seccomp_notif_resp::default()).unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().raw_os_error(),
            Some(libc::EBADF)
        );
        assert!(!is_id_valid(-1, 123));
    }
}
