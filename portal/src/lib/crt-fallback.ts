export type CRTHandle = {
    update: (text: string) => void;
    dispose: () => void;
};
export function createCRT(host: HTMLElement, initial: string, reduced: boolean): CRTHandle {
    const canvas = document.createElement('canvas');
    canvas.width = 1200;
    canvas.height = 700;
    canvas.setAttribute('aria-hidden', 'true');
    host.append(canvas);
    const ctx = canvas.getContext('2d');
    let text = initial, frame = 0, tear = 0;
    function draw() { if (!ctx)
        return; ctx.fillStyle = '#071d1a'; ctx.fillRect(0, 0, 1200, 700); ctx.fillStyle = '#8ee8be'; ctx.font = '22px monospace'; text.split('\n').forEach((line, i) => ctx.fillText(line, 55 + (tear > 0 ? Math.sin(i * 8) * tear : 0), 65 + i * 39)); ctx.fillStyle = 'rgba(0,0,0,.22)'; for (let y = 0; y < 700; y += 4)
        ctx.fillRect(0, y, 1200, 1); const v = ctx.createRadialGradient(600, 350, 50, 600, 350, 710); v.addColorStop(0, 'transparent'); v.addColorStop(1, '#000b'); ctx.fillStyle = v; ctx.fillRect(0, 0, 1200, 700); tear *= .84; if (tear > 0.2 && !reduced)
        frame = requestAnimationFrame(draw); }
    draw();
    return { update: (s) => { text = s; tear = reduced ? 0 : 25; cancelAnimationFrame(frame); draw(); }, dispose: () => { cancelAnimationFrame(frame); canvas.remove(); } };
}
