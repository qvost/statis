import research from '@/generated/research.json';
export { research };
export const repo = 'https://github.com/qvost/statis';
export const sourceLink = (path: string) => `${repo}/blob/main/${path}`;
export const modules = [
    { name: 'Sterile Launcher', tag: 'ISOLATE', paths: ['src/launcher/mod.rs', 'src/launcher/auxv.rs', 'src/launcher/fd_pass.rs'], description: 'Request disabled ASLR, pin CPU 0, transfer the listener, then scrub the vDSO at the exec stop.', symbol: 'launch_child_with_options', detail: 'AT_SYSINFO_EHDR (33) → AT_IGNORE (1)' },
    { name: 'Seccomp BPF Filter', tag: 'INTERCEPT', paths: ['src/interceptor/bpf.rs', 'src/interceptor/notif.rs'], description: 'Classic BPF routes selected syscalls to userspace notifications. Unmatched syscalls continue in the kernel.', symbol: 'create_filter_prog_with_io', detail: '16 base syscalls · 9 optional I/O syscalls' },
    { name: 'Priority Time-Wheel', tag: 'ADVANCE', paths: ['src/clock/mod.rs', 'src/clock/wheel.rs'], description: 'A BinaryHeap orders deadlines, with deterministic ties. The monotonic clock advances when work becomes idle.', symbol: 'TimeWheel', detail: '100 ns clock tick · inclusive deadlines' },
    { name: 'TSC & Entropy Trap', tag: 'EMULATE', paths: ['src/interceptor/tsc.rs', 'src/supervisor.rs'], description: 'Decode timestamp instructions at SIGSEGV stops. getrandom fills buffers from a seeded ChaCha20 stream.', symbol: 'handle_tsc_fault', detail: 'RDTSC / RDTSCP +50 ns · synthetic core 0' },
    { name: 'Deterministic Faults', tag: 'PERTURB', paths: ['src/interceptor/fault.rs'], description: 'Eligible I/O consumes seeded draws; configured errors return negative errno. read/write on fd ≤ 2 stay protected.', symbol: 'FaultConfig::should_fault', detail: '8 errno variants · no RNG draw for protected calls' },
    { name: 'Futex & ID Virtualizer', tag: 'COORDINATE', paths: ['src/interceptor/sync.rs', 'src/interceptor/pid.rs', 'src/interceptor/virtual_ids.rs'], description: 'FIFO wait queues and virtual deadlines meet a tracked process tree. Compatibility exports live in virtual_ids.', symbol: 'FutexManager / VirtualIdManager', detail: 'Root PID 1000 · PPID 999 · virtual-address keys' },
    { name: 'PMU Scheduler & Trace', tag: 'VERIFY', paths: ['src/scheduler/mod.rs', 'src/scheduler/pmu.rs', 'src/trace/format.rs', 'src/vm.rs'], description: 'Seeded runnable selection, experimental perf counters, and versioned trace verification. Remote transfers must complete.', symbol: 'DeterministicScheduler / TraceWriter', detail: '64 KiB trace buffers · PMU preemption partial' },
];
export const fixtureNotes: Record<string, {
    title: string;
    description: string;
    rules: string;
    stdout: string;
}> = {
    test_target: { title: 'Clock & entropy', description: 'Two threads each read 16 random bytes, query CLOCK_MONOTONIC twice, and sleep for 100 ms.', rules: 'getrandom · clock_gettime · nanosleep · futex', stdout: '[Target Thread {id}] getrandom: {32 hex digits}\n[Target Thread {id}] time: {seconds}s {nanoseconds}ns' },
    test_timewarp: { title: 'Deadline compression', description: 'A ten-second nanosleep, with an optional whole-second duration for the benchmark sweep.', rules: 'nanosleep / clock_nanosleep · TimeWheel', stdout: '[TimeWarp Target] Starting 10-second nanosleep...\n[TimeWarp Target] Woke up successfully!' },
    test_concurrency: { title: 'Thread contention', description: 'Four threads each acquire a mutex five times. Reports all 20 acquisitions and the final counter.', rules: 'futex · sched_yield · seeded runnable selection', stdout: '[Concurrency Target] Lock acquisition order: {20 entries}\n[Concurrency Target] Final counter: 20' },
    test_rdtsc: { title: 'Hardware timestamps', description: 'One RDTSC read followed by RDTSCP. Asserts that the second counter exceeds the first.', rules: 'PR_SET_TSC · SIGSEGV · RAX:RDX · RCX = 0', stdout: '[RDTSC Target] Querying hardware timestamp counter...\n[RDTSC Target] First read:  {t1}\n[RDTSC Target] Second read: {t2}\n[RDTSC Target] Synthetic core ID (aux): {aux}\n[RDTSC Target] RDTSC emulation verified successfully!' },
    test_fault_injection: { title: 'Seeded I/O chaos', description: 'Exercises I/O boundaries under configured failures. Console read/write file descriptors are protected.', rules: '--fault-rate · --fault-types · fd ≤ 2 protection', stdout: '[Fault Target] Operations: 50 | Successes: {successes} | Injected faults: {count} (codes: {errno array})' },
    test_epoll: { title: 'Finite event timeout', description: 'An epoll instance watching an idle nonblocking pipe waits 15,000 ms. The supervised path synthesizes timeout completion.', rules: 'epoll_wait · finite timeout · readiness outside emulation', stdout: '[Epoll Target] Calling epoll_wait with 15,000ms timeout...\n[Epoll Target] epoll_wait returned: 0 (expected 0 for timeout)' },
    test_record_replay: { title: 'Trace verification', description: 'Three rounds of clock, 16 random bytes, a 1 ms sleep, and RDTSC feed an FNV-style state hash.', rules: 'record · replay · trace-dump · semantic argument matching', stdout: '[RecordReplay Target] State hash: {16 hex digits}' },
    test_process_tree: { title: 'Process identities', description: 'Verifies the root PID/PPID, then child identities 1001 and 1002 with parent 1000.', rules: 'getpid · getppid · fork · virtual ID translation', stdout: '[ProcessTree] Child 1 PID 1001 verified\n[ProcessTree] All hierarchy tests passed deterministically' },
};
export const vectors = [
    ['Temporal / vDSO', 'Auxv scrubbing routes loader clock reads through syscalls. VirtualClock advances 100 ns per clock read.'],
    ['Architectural / RDTSC', 'PR_SET_TSC traps timestamp instructions; the supervisor emulates the counter and advances 50 ns.'],
    ['Environmental / getrandom', 'ChaCha20 supplies intercepted getrandom buffers. Reads from /dev/urandom remain outside this emulation.'],
    ['Spatial / ASLR', 'The launcher requests ADDR_NO_RANDOMIZE. This requires host permission; memory layout is not restored by replay.'],
    ['Identifier / PID & TID', 'Virtual IDs track traced tasks. The root starts at PID 1000 and PPID 999; hierarchy semantics need broader validation.'],
];
export const limits = [['PMU preemption is partial', 'The interface and instruction quantum exist; the supervisor does not enforce a complete instruction-preemption loop.'], ['Timeout completion ≠ I/O readiness', 'Finite poll/epoll emulation completes timeouts; it does not simulate descriptor readiness.'], ['Virtual-address futex keys', 'Different processes can reuse the same address. A process-aware key is still needed; thread-exit wakeups use a heuristic.'], ['Replay checks selected fields', 'The trace does not restore all target memory writes or capture every kernel-side I/O result.'], ['Scheduling & identities', 'Arbitrary userspace interleavings, signal translation, process groups, and separate PID/TID counters still need integration validation.']];
