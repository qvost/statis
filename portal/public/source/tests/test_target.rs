use std::thread;

fn run_target_thread(id: u8) {
    let mut random_bytes = [0_u8; 16];
    // SAFETY: the buffer is writable and exactly 16 bytes long.
    let random_result =
        unsafe { libc::getrandom(random_bytes.as_mut_ptr().cast(), random_bytes.len(), 0) };
    if random_result < 0 {
        eprintln!("[Target Thread {id}] getrandom failed");
    } else {
        let hex: String = random_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        println!("[Target Thread {id}] getrandom: {hex}");
    }

    for _ in 0..2 {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` points to valid writable storage.
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } == 0 {
            println!("[Target Thread {id}] time: {}s {}ns", ts.tv_sec, ts.tv_nsec);
        } else {
            eprintln!("[Target Thread {id}] clock_gettime failed");
        }

        for n in 0_u64..10_000 {
            std::hint::black_box(n.wrapping_mul(n));
        }
    }

    let request = libc::timespec {
        tv_sec: 0,
        tv_nsec: 100_000_000,
    };
    // SAFETY: `request` is a valid timespec; no remainder is needed here.
    unsafe {
        libc::nanosleep(&request, std::ptr::null_mut());
    }
}

fn main() {
    let first = thread::spawn(|| run_target_thread(1));
    let second = thread::spawn(|| run_target_thread(2));
    first.join().expect("target thread 1 panicked");
    second.join().expect("target thread 2 panicked");
}
