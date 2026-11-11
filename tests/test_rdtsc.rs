fn main() {
    println!("[RDTSC Target] Querying hardware timestamp counter...");

    // First read using core::arch::x86_64::_rdtsc
    let t1 = unsafe { core::arch::x86_64::_rdtsc() };

    // Small computation to advance time
    let mut sum = 0_u64;
    for i in 1..10_000 {
        sum = sum.wrapping_add(i * i);
    }
    std::hint::black_box(sum);

    // Second read using core::arch::x86_64::__rdtscp
    let mut aux = 0_u32;
    let t2 = unsafe { core::arch::x86_64::__rdtscp(&mut aux) };

    println!("[RDTSC Target] First read:  {t1}");
    println!("[RDTSC Target] Second read: {t2}");
    println!("[RDTSC Target] Synthetic core ID (aux): {aux}");

    if t2 <= t1 {
        eprintln!("[RDTSC Target] Error: cycle counter did not advance! ({t2} <= {t1})");
        std::process::exit(1);
    }

    println!("[RDTSC Target] RDTSC emulation verified successfully!");
}
