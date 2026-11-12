// Rust rand_core 0.6 seed_from_u64 + rand_chacha 0.3 ChaCha20Rng compatibility.
const MASK64 = (1n << 64n) - 1n;
export function parseSeed(value: string): bigint | null { try {
    if (!/^(?:0x[\da-f]+|\d+)$/i.test(value))
        return null;
    const n = BigInt(value);
    return n >= 0n && n <= MASK64 ? n : null;
}
catch {
    return null;
} }
const rotl = (a: number, n: number) => (a << n) | (a >>> (32 - n));
export function chaCha20(seed: bigint) {
    const key: number[] = [];
    let state = seed;
    for (let i = 0; i < 8; i++) {
        state = (state * 6364136223846793005n + 11634580027462260723n) & MASK64;
        const x = Number((((state >> 18n) ^ state) >> 27n) & 0xffffffffn), r = Number(state >> 59n);
        key.push(((x >>> r) | (x << ((-r) & 31))) >>> 0);
    }
    let counter = 0n, buffer: number[] = [], offset = 0;
    function block() {
        const initial = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574, ...key, Number(counter & 0xffffffffn), Number(counter >> 32n), 0, 0], x = initial.slice();
        counter++;
        const q = (a: number, b: number, c: number, d: number) => { x[a] = (x[a] + x[b]) | 0; x[d] = rotl(x[d] ^ x[a], 16); x[c] = (x[c] + x[d]) | 0; x[b] = rotl(x[b] ^ x[c], 12); x[a] = (x[a] + x[b]) | 0; x[d] = rotl(x[d] ^ x[a], 8); x[c] = (x[c] + x[d]) | 0; x[b] = rotl(x[b] ^ x[c], 7); };
        for (let i = 0; i < 10; i++) {
            q(0, 4, 8, 12);
            q(1, 5, 9, 13);
            q(2, 6, 10, 14);
            q(3, 7, 11, 15);
            q(0, 5, 10, 15);
            q(1, 6, 11, 12);
            q(2, 7, 8, 13);
            q(3, 4, 9, 14);
        }
        buffer = x.map((v, i) => (v + initial[i]) >>> 0);
        offset = 0;
    }
    const u32 = () => { if (offset === buffer.length)
        block(); return buffer[offset++]; };
    return () => BigInt(u32()) | (BigInt(u32()) << 32n);
}
export function faultPreview(seed: bigint, rate: number, types: string[], count = 128) { const next = chaCha20(seed); return Array.from({ length: count }, () => { if (rate <= 0 || !types.length)
    return 'ok'; return Number(next()) / Number(MASK64) < rate ? types[Number(next() % BigInt(types.length))] : 'ok'; }); }
export function seededOrder(seed: bigint, count = 150) { const next = chaCha20(seed), items = Array.from({ length: count }, (_, i) => i); for (let i = count - 1; i > 0; i--) {
    const j = Number(next() % BigInt(i + 1));
    [items[i], items[j]] = [items[j], items[i]];
} return items; }
export type CommandMode = 'run' | 'record' | 'replay' | 'trace-dump';
export function buildCommand(mode: CommandMode, fixture: string, seed: bigint, quantum: number, rate: number, types: string[], summary: boolean) {
    if (mode === 'trace-dump')
        return 'statis trace-dump exec.trace';
    const target = `-- ./target/release/${fixture}`;
    if (mode === 'replay')
        return `statis replay --trace exec.trace ${target}`;
    return `statis ${mode}${mode === 'record' ? ' --trace exec.trace' : ''} --seed ${seed} --quantum ${quantum}${rate > 0 ? ` --fault-rate ${rate} --fault-types ${types.join(',')}` : ''}${mode === 'run' && !summary ? ' --summary=false' : ''} ${target}`;
}
