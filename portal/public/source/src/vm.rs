//! Cross-process memory transfers. Successful calls transfer the entire value.

use std::{
    io::{IoSlice, IoSliceMut},
    mem::{MaybeUninit, size_of},
};

use nix::{
    errno::Errno,
    sys::uio::{RemoteIoVec, process_vm_readv, process_vm_writev},
    unistd::Pid,
};

fn check_range(remote_addr: usize, len: usize) -> nix::Result<()> {
    if len > isize::MAX as usize || remote_addr.checked_add(len).is_none() {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// Read exactly `len` bytes, returning EFAULT for a partial transfer.
pub fn read_remote_bytes(pid: Pid, remote_addr: usize, len: usize) -> nix::Result<Vec<u8>> {
    check_range(remote_addr, len)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Errno::ENOMEM)?;
    bytes.resize(len, 0);
    if len == 0 {
        return Ok(bytes);
    }
    let remote = [RemoteIoVec {
        base: remote_addr,
        len,
    }];
    loop {
        match process_vm_readv(pid, &mut [IoSliceMut::new(&mut bytes)], &remote) {
            Ok(count) if count == len => return Ok(bytes),
            Ok(_) => return Err(Errno::EFAULT),
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Read a native value whose type permits every bit pattern.
pub fn read_remote_val<T: bytemuck::AnyBitPattern>(pid: Pid, remote_addr: usize) -> nix::Result<T> {
    let len = size_of::<T>();
    check_range(remote_addr, len)?;
    let mut val = MaybeUninit::<T>::uninit();
    if len == 0 {
        // SAFETY: a zero-sized AnyBitPattern value needs no initialized bytes.
        return Ok(unsafe { val.assume_init() });
    }
    // Raw iovecs let the kernel initialize stack storage without creating a
    // Rust byte slice over uninitialized memory.
    let local = libc::iovec {
        iov_base: val.as_mut_ptr().cast(),
        iov_len: len,
    };
    let remote = libc::iovec {
        iov_base: remote_addr as *mut libc::c_void,
        iov_len: len,
    };
    loop {
        // SAFETY: local covers writable storage for T. The kernel validates the
        // remote address and reports the exact number of bytes initialized.
        let count = unsafe { libc::process_vm_readv(pid.as_raw(), &local, 1, &remote, 1, 0) };
        if count >= 0 {
            if count as usize != len {
                return Err(Errno::EFAULT);
            }
            // SAFETY: all bytes were initialized; AnyBitPattern accepts them.
            return Ok(unsafe { val.assume_init() });
        }
        let error = Errno::last();
        if error != Errno::EINTR {
            return Err(error);
        }
    }
}

/// Write all bytes, returning EFAULT for a partial transfer.
/// A failed call may already have modified part of the remote memory.
pub fn write_remote_bytes(pid: Pid, remote_addr: usize, data: &[u8]) -> nix::Result<()> {
    check_range(remote_addr, data.len())?;
    if data.is_empty() {
        return Ok(());
    }
    let remote = [RemoteIoVec {
        base: remote_addr,
        len: data.len(),
    }];
    loop {
        match process_vm_writev(pid, &[IoSlice::new(data)], &remote) {
            Ok(count) if count == data.len() => return Ok(()),
            Ok(_) => return Err(Errno::EFAULT),
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Write the native memory representation of a copyable value.
/// Both processes must agree on its layout and ABI.
pub fn write_remote_val<T: Copy>(pid: Pid, remote_addr: usize, val: &T) -> nix::Result<()> {
    let len = size_of::<T>();
    check_range(remote_addr, len)?;
    if len == 0 {
        return Ok(());
    }
    // Pass the representation directly to the kernel. Unlike constructing a
    // &[u8], this also supports Copy types containing uninitialized padding.
    let local = libc::iovec {
        iov_base: std::ptr::from_ref(val).cast_mut().cast(),
        iov_len: len,
    };
    let remote = libc::iovec {
        iov_base: remote_addr as *mut libc::c_void,
        iov_len: len,
    };
    loop {
        // SAFETY: local covers the live value; the kernel validates the remote
        // address. Neither iovec nor the local value is modified by this call.
        let result = unsafe { libc::process_vm_writev(pid.as_raw(), &local, 1, &remote, 1, 0) };
        if result >= 0 {
            return if result as usize == len {
                Ok(())
            } else {
                Err(Errno::EFAULT)
            };
        }
        let error = Errno::last();
        if error != Errno::EINTR {
            return Err(error);
        }
    }
}

#[cfg(test)]
pub(crate) fn memory_access_available() -> bool {
    let mut byte = 0_u8;
    let address = std::ptr::from_mut(&mut byte) as usize;
    let result = write_remote_bytes(Pid::this(), address, &[1])
        .and_then(|()| read_remote_bytes(Pid::this(), address, 1).map(|_| ()));
    match result {
        Ok(()) => true,
        Err(Errno::EPERM | Errno::EACCES | Errno::ENOSYS) => {
            eprintln!("skipping memory injection assertions: process_vm access is unavailable");
            false
        }
        Err(error) => panic!("unexpected process_vm probe error: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_reads_accept_unaligned_input_and_reject_partial_values() {
        if !memory_access_available() {
            return;
        }
        let bytes = [0_u8, 1, 2, 3, 4];
        assert_eq!(
            read_remote_val::<u32>(Pid::this(), bytes[1..].as_ptr() as usize).unwrap(),
            u32::from_ne_bytes([1, 2, 3, 4])
        );
        // SAFETY: sysconf has no pointer arguments.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(page > 0);
        let page = page as usize;
        struct Mapping(*mut libc::c_void, usize);
        impl Drop for Mapping {
            fn drop(&mut self) {
                // SAFETY: this mapping was allocated below with this length.
                unsafe {
                    libc::munmap(self.0, self.1);
                }
            }
        }
        // SAFETY: anonymous private mapping, no file descriptor or input memory.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                page * 2,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(ptr, libc::MAP_FAILED);
        let mapping = Mapping(ptr, page * 2);
        // SAFETY: second_page is an aligned page wholly inside this mapping.
        let second_page = unsafe { mapping.0.cast::<u8>().add(page) };
        assert_eq!(
            unsafe { libc::mprotect(second_page.cast(), page, libc::PROT_NONE) },
            0
        );
        let address = second_page as usize - size_of::<u64>();
        // The first word is readable; the second lies in the protected page.
        assert_eq!(
            read_remote_val::<[u64; 2]>(Pid::this(), address),
            Err(Errno::EFAULT)
        );
        assert_eq!(
            read_remote_bytes(Pid::this(), address, 16),
            Err(Errno::EFAULT)
        );
    }

    #[test]
    fn memory_round_trip() {
        if !memory_access_available() {
            return;
        }
        let mut buffer = [0_u8; 8];
        let address = buffer.as_mut_ptr() as usize;
        write_remote_bytes(Pid::this(), address, b"statis!!").unwrap();
        assert_eq!(
            read_remote_bytes(Pid::this(), address, 8).unwrap(),
            b"statis!!"
        );
        let value = 0x1234_5678_9abc_def0_u64;
        write_remote_val(Pid::this(), address, &value).unwrap();
        assert_eq!(buffer, value.to_ne_bytes());
        assert_eq!(read_remote_val::<u64>(Pid::this(), address).unwrap(), value);
    }

    #[test]
    fn empty_and_invalid_ranges() {
        assert_eq!(read_remote_bytes(Pid::this(), 0, 0).unwrap(), []);
        write_remote_bytes(Pid::this(), 0, &[]).unwrap();
        write_remote_val(Pid::this(), 0, &()).unwrap();
        assert_eq!(read_remote_val::<[u8; 0]>(Pid::this(), 0).unwrap(), []);
        assert_eq!(
            read_remote_val::<u64>(Pid::this(), usize::MAX),
            Err(Errno::EFAULT)
        );
        assert_eq!(
            write_remote_bytes(Pid::this(), usize::MAX, &[1]),
            Err(Errno::EFAULT)
        );
        if memory_access_available() {
            assert_eq!(read_remote_bytes(Pid::this(), 0, 1), Err(Errno::EFAULT));
            assert_eq!(write_remote_bytes(Pid::this(), 0, &[1]), Err(Errno::EFAULT));
        }
    }
}
