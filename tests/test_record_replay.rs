fn main() {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    for _ in 0..3 {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: ts points to valid writable storage.
        unsafe {
            libc::syscall(
                libc::SYS_clock_gettime,
                libc::CLOCK_MONOTONIC,
                &mut ts as *mut libc::timespec,
            );
        }
        state = hash_bytes(state, &ts.tv_sec.to_ne_bytes());
        state = hash_bytes(state, &ts.tv_nsec.to_ne_bytes());

        let mut random = [0_u8; 16];
        // SAFETY: random is a valid writable buffer.
        unsafe {
            libc::syscall(libc::SYS_getrandom, random.as_mut_ptr(), random.len(), 0);
        }
        state = hash_bytes(state, &random);

        let request = libc::timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000,
        };
        // SAFETY: request points to a valid timespec.
        unsafe {
            libc::syscall(
                libc::SYS_nanosleep,
                &request as *const libc::timespec,
                std::ptr::null_mut::<libc::timespec>(),
            );
        }

        // SAFETY: rdtsc is available on x86-64.
        #[cfg(target_arch = "x86_64")]
        let cycles = unsafe { core::arch::x86_64::_rdtsc() };
        #[cfg(not(target_arch = "x86_64"))]
        let cycles = 0_u64;
        state = hash_bytes(state, &cycles.to_ne_bytes());
    }
    println!("[RecordReplay Target] State hash: {state:016x}");
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
