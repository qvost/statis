use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let trace = Arc::new(Mutex::new(Vec::<u32>::new()));
    let counter = Arc::new(Mutex::new(0_u64));
    let mut workers = Vec::with_capacity(4);

    for id in 0..4_u32 {
        let trace = Arc::clone(&trace);
        let counter = Arc::clone(&counter);
        workers.push(thread::spawn(move || {
            for _ in 0..5 {
                let mut trace = trace.lock().expect("trace mutex poisoned");
                let mut counter = counter.lock().expect("counter mutex poisoned");
                trace.push(id);
                *counter += 1;
                thread::yield_now();
            }
        }));
    }

    for worker in workers {
        worker.join().expect("worker thread panicked");
    }

    let trace = trace.lock().expect("trace mutex poisoned");
    let counter = counter.lock().expect("counter mutex poisoned");
    println!("[Concurrency Target] Lock acquisition order: {trace:?}");
    println!("[Concurrency Target] Final counter: {counter}");
}
