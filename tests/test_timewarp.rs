fn main() {
    // Keep the ten-second fixture default; allow the benchmark duration sweep.
    let seconds = std::env::args()
        .nth(1)
        .map(|value| {
            value
                .parse::<i64>()
                .expect("duration must be whole seconds")
        })
        .unwrap_or(10);
    assert!(seconds > 0, "duration must be positive");
    println!("[TimeWarp Target] Starting {seconds}-second nanosleep...");

    let request = libc::timespec {
        tv_sec: seconds,
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
