fn main() {
    println!("[TimeWarp Target] Starting 10-second nanosleep...");

    let request = libc::timespec {
        tv_sec: 10,
        tv_nsec: 0,
    };
    // SAFETY: `request` is a valid timespec and no remainder is needed.
    let result = unsafe { libc::nanosleep(&request, std::ptr::null_mut()) };
    if result != 0 {
        eprintln!("[TimeWarp Target] nanosleep failed");
        std::process::exit(1);
    }

    println!("[TimeWarp Target] Woke up successfully!");
}
