const syscallNames: Record<number, string> = { 0: 'read', 1: 'write', 7: 'poll', 24: 'sched_yield', 35: 'nanosleep', 39: 'getpid', 42: 'connect', 43: 'accept', 44: 'sendto', 45: 'recvfrom', 46: 'sendmsg', 47: 'recvmsg', 62: 'kill', 96: 'gettimeofday', 110: 'getppid', 186: 'gettid', 202: 'futex', 228: 'clock_gettime', 230: 'clock_nanosleep', 232: 'epoll_wait', 234: 'tgkill', 271: 'ppoll', 281: 'epoll_pwait', 288: 'accept4', 318: 'getrandom' };
export function decodeTrace(buffer: ArrayBuffer) {
    if (buffer.byteLength > 8 * 1024 * 1024)
        throw new Error('Trace inspection is limited to 8 MiB. Use statis trace-dump for larger files.');
    const bytes = new Uint8Array(buffer), view = new DataView(buffer);
    const magic = [83, 84, 65, 84, 73, 83, 0, 1];
    if (magic.some((b, i) => bytes[i] !== b))
        throw new Error('Expected STATIS\\0\\x01 version 1 header.');
    let offset = 8;
    const ensure = (n: number) => { if (n < 0 || offset + n > buffer.byteLength)
        throw new Error(`Truncated trace at byte ${offset}.`); };
    const u32 = () => { ensure(4); const v = view.getUint32(offset, true); offset += 4; return v; };
    const i32 = () => { ensure(4); const v = view.getInt32(offset, true); offset += 4; return v; };
    const u64 = () => { ensure(8); const v = view.getBigUint64(offset, true); offset += 8; return v; };
    const i64 = () => { ensure(8); const v = view.getBigInt64(offset, true); offset += 8; return v; };
    const seed = u64(), quantum = u64();
    ensure(8);
    const rate = view.getFloat64(offset, true);
    offset += 8;
    if (!Number.isFinite(rate) || rate < 0 || rate > 1)
        throw new Error('Invalid trace fault rate.');
    const count = u32();
    if (count > 100000)
        throw new Error('Unreasonable command argument count.');
    const command = [];
    for (let i = 0; i < count; i++) {
        const n = u32();
        ensure(n);
        command.push(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(offset, offset + n)));
        offset += n;
    }
    const lines = ['statis trace v1', `seed: ${seed}`, `quantum: ${quantum}`, `fault_rate: ${rate}`, `command: ${JSON.stringify(command)}`];
    let now = 0n, events = 0;
    while (offset < buffer.byteLength) {
        if (events >= 5000) {
            lines.push('… Display limited to 5,000 events. Use the CLI for a complete dump.');
            break;
        }
        ensure(1);
        const tag = bytes[offset++], tid = u32();
        if (tag === 1) {
            const nr = i32(), args = Array.from({ length: 6 }, () => u64());
            now = u64();
            const ret = i64(), error = i32();
            lines.push(`[${now} ns] tid=${tid} SYS_${syscallNames[nr] || nr}(${args.map(v => '0x' + v.toString(16)).join(', ')}) -> ${ret} (error: ${error})`);
        }
        else if (tag === 2) {
            now = u64();
            lines.push(`[${now} ns] tid=${tid} RDTSC -> ${now} cycles`);
        }
        else if (tag === 3) {
            lines.push(`[${now} ns] tid=${tid} EXIT -> code ${i32()}`);
        }
        else
            throw new Error(`Unknown event tag ${tag} at byte ${offset - 5}.`);
        events++;
    }
    return { text: lines.join('\n'), events, bytes: buffer.byteLength };
}
