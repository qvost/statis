'use client';
import { useEffect, useRef, useState } from 'react';
import { sourceLink } from '@/lib/research';
const vertex = `#version 300 es
in vec2 position;out vec2 uv;void main(){uv=position*.5+.5;gl_Position=vec4(position,0.,1.);}`;
const fragment = `#version 300 es
precision highp float;in vec2 uv;out vec4 color;uniform sampler2D previous;uniform vec2 resolution;uniform vec2 pointer;uniform float time;uniform int stage;
vec3 sampleAt(vec2 p){return texture(previous,clamp(p,vec2(0.),vec2(1.))).rgb;}
void main(){vec2 p=uv;vec2 texel=1./resolution;
if(stage==0){color=vec4(.82,.85,.72,1.);return;}
if(stage==1){float d=length((p-pointer)*vec2(resolution.x/resolution.y,1.));float a=smoothstep(.4,.01,d);color=vec4(mix(sampleAt(p),vec3(.30,.37,.26),a*.7),1.);return;}
if(stage==2){vec2 warp=vec2(sin(p.y*18.+time),cos(p.x*19.-time))*.022;vec3 c=sampleAt(p+warp);c.r=sampleAt(p+warp+vec2(.008,0.)).r;c.b=sampleAt(p+warp-vec2(.008,0.)).b;color=vec4(c,1.);return;}
if(stage==3||stage==4){vec2 d=stage==3?vec2(texel.x*3.,0.):vec2(0.,texel.y*3.);color=vec4(sampleAt(p)*.4+(sampleAt(p-d)+sampleAt(p+d))*.24+(sampleAt(p-d*2.)+sampleAt(p+d*2.))*.06,1.);return;}
p+=vec2(sin(p.y*24.+time*2.),cos(p.x*26.-time*1.5))*.009;color=vec4(sampleAt(p),1.);}`;
function mountLiquid(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, powerPreference: 'low-power' });
    if (!gl)
        return null;
    const shader = (type: number, src: string) => { const s = gl.createShader(type)!; gl.shaderSource(s, src); gl.compileShader(s); if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
        gl.deleteShader(s);
        throw new Error('Liquid shader unavailable');
    } return s; };
    const vs = shader(gl.VERTEX_SHADER, vertex), fs = shader(gl.FRAGMENT_SHADER, fragment), program = gl.createProgram()!;
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS))
        return null;
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
    const attr = gl.getAttribLocation(program, 'position');
    gl.enableVertexAttribArray(attr);
    gl.vertexAttribPointer(attr, 2, gl.FLOAT, false, 0, 0);
    const textures: WebGLTexture[] = [], targets: WebGLFramebuffer[] = [];
    let w = 0, h = 0;
    const resize = () => {
        const rect = canvas.getBoundingClientRect();
        const scale = Math.min(1, 960 / Math.max(1, rect.width), 480 / Math.max(1, rect.height));
        const nextWidth = Math.max(1, Math.round(rect.width * scale));
        const nextHeight = Math.max(1, Math.round(rect.height * scale));
        if (nextWidth === w && nextHeight === h) return;
        w = nextWidth; h = nextHeight; canvas.width = w; canvas.height = h;
        for (const texture of textures) {
            gl.bindTexture(gl.TEXTURE_2D, texture);
            gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
        }
    };
    for (let i = 0; i < 2; i++) {
        const texture = gl.createTexture()!, target = gl.createFramebuffer()!;
        textures.push(texture);
        targets.push(target);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.bindFramebuffer(gl.FRAMEBUFFER, target);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0);
    }
    const location = (s: string) => gl.getUniformLocation(program, s);
    const stage = location('stage'), resolution = location('resolution'), pointer = location('pointer'), time = location('time');
    gl.uniform1i(location('previous'), 0);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    return { draw: (t: number, x: number, y: number) => { gl.useProgram(program); gl.viewport(0, 0, w, h); gl.uniform2f(resolution, w, h); gl.uniform2f(pointer, x, y); gl.uniform1f(time, t / 1000); for (let i = 0; i < 6; i++) {
            gl.bindFramebuffer(gl.FRAMEBUFFER, i === 5 ? null : targets[i % 2]);
            gl.activeTexture(gl.TEXTURE0);
            gl.bindTexture(gl.TEXTURE_2D, textures[(i + 1) % 2]);
            gl.uniform1i(stage, i);
            gl.drawArrays(gl.TRIANGLES, 0, 6);
        } }, dispose: () => { observer.disconnect(); textures.forEach(t => gl.deleteTexture(t)); targets.forEach(t => gl.deleteFramebuffer(t)); gl.deleteBuffer(buffer); gl.deleteProgram(program); gl.deleteShader(vs); gl.deleteShader(fs); } };
}
export function LiquidStatGrid({ showLabels = true }: { showLabels?: boolean }) {
    const ref = useRef<HTMLCanvasElement>(null);
    const [always, setAlways] = useState(false);
    const pointer = useRef({ x: .5, y: .5 });
    useEffect(() => {
        if (!always || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
        let frame = 0, visible = false, previous = 0;
        let liquid: ReturnType<typeof mountLiquid> = null;
        try { liquid = mountLiquid(ref.current!); } catch { return; }
        if (!liquid) return;
        const draw = (time: number) => {
            frame = 0;
            if (!visible || document.hidden) return;
            if (!previous || time - previous >= 50) {
                previous = time; liquid!.draw(time, pointer.current.x, pointer.current.y);
            }
            frame = requestAnimationFrame(draw);
        };
        const restart = () => {
            cancelAnimationFrame(frame); frame = 0; previous = 0;
            if (visible && !document.hidden) frame = requestAnimationFrame(draw);
        };
        const observer = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; restart(); });
        observer.observe(ref.current!); document.addEventListener('visibilitychange', restart);
        return () => {
            cancelAnimationFrame(frame); observer.disconnect();
            document.removeEventListener('visibilitychange', restart); liquid?.dispose();
        };
    }, [always]);
    return <section className="stat-section section" data-surface="white"><div className={showLabels ? "section-kicker" : "section-tools"}>{showLabels ? <span>01 / RESEARCH IN NUMBERS</span> : <h2 className="sr-only">Research metrics</h2>}<label className="toggle-label"><input type="checkbox" checked={always} onChange={e => setAlways(e.target.checked)}/> Enable liquid effect</label></div><div className="liquid-stat-grid" onPointerMove={e => { if (!always) return; const r = e.currentTarget.getBoundingClientRect(); pointer.current = { x: (e.clientX - r.left) / r.width, y: 1 - (e.clientY - r.top) / r.height }; }}><canvas ref={ref} className="liquid-canvas" aria-hidden="true" style={{ visibility: always ? 'visible' : 'hidden' }}/>{[
            { metric: <>100 <small>ns</small></>, title: 'Virtual Clock Quantum', badge: 'IMPLEMENTED / SOURCE', description: 'Clock reads +100 ns. RDTSC / RDTSCP +50 ns. Root PID 1000 / PPID 999.', path: 'src/clock/mod.rs', note: 'Deadline heap: clock::wheel' },
            { metric: <>872<small>× Ref</small></>, title: 'Time-Warp Illustration', badge: 'REFERENCE / PAPER: 556×–882×', description: 'SVG reference: 10,000 ms → 11.47 ms. 7-page paper reports 18 ms (556×) & 17 ms epoll (882×). Saved sandbox harness: native 10.001 s, supervised incomplete.', path: 'docs/benchmarking.md', note: 'Chart baseline + 7-page paper context' },
            { metric: <><span className="storage-metric">45.2 MB</span><span className="storage-to">→ 0.14 KB</span></>, title: 'Supplied Storage Reference', badge: 'REFERENCE SCENARIO', description: 'rr 45,200 KB / Hermit 1,250 KB / statis 0.14 KB. Unmatched workloads. Current traces have no delta compression.', path: 'docs/benchmarking.md', note: 'Decimal KB · 0.14 KB = 140 bytes' },
        ].map(s => <article key={s.title} className="stat-cell">{showLabels && <span className="provenance">{s.badge}</span>}<div className="stat-value">{s.metric}</div><h3>{s.title}</h3><p>{s.description}</p><a className="source-note" href={sourceLink(s.path)}>{s.note} ↗</a></article>)}</div></section>;
}
