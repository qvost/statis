'use client';
import { useEffect, useState, useRef, useId } from 'react';
export const orbitTerms = ['SECCOMP-BPF', 'VIRTUAL CLOCK', 'CHACHA20 PRNG', 'VDSO STRIP', 'RDTSC TRAP', 'FUTEX QUEUE', 'TIME WARPING', 'TRACE STREAM'];
export function OrbitTextPreloader({ onComplete, duration = 2400 }: {
    onComplete: () => void;
    duration?: number;
}) {
    const [count, setCount] = useState(0);
    const id = useId().replace(/:/g, ''), done = useRef(false);
    useEffect(() => { if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
        onComplete();
        return;
    } let frame = 0, timer: ReturnType<typeof setTimeout>; const start = performance.now(); const loop = (now: number) => { const p = Math.min(100, Math.floor((now - start) / duration * 100)); setCount(p); if (p === 100) {
        timer = setTimeout(onComplete, 130);
    }
    else
        frame = requestAnimationFrame(loop); }; frame = requestAnimationFrame(loop); const safety = setTimeout(onComplete, duration + 1800); return () => { cancelAnimationFrame(frame); clearTimeout(timer); clearTimeout(safety); }; }, [duration, onComplete]);
    return <div className="orbit-preloader" role="dialog" aria-modal="true" aria-label="Statis boot sequence" onKeyDown={e => { if (e.key === 'Tab') {
        e.preventDefault();
        e.currentTarget.querySelector<HTMLButtonElement>('button')?.focus();
    } if (e.key === 'Escape')
        onComplete(); }}><div className="boot-top mono"><span>STATIS / RESEARCH RUNTIME</span><button autoFocus onClick={() => { if (!done.current) {
        done.current = true;
        setCount(100);
        onComplete();
    } }}>Skip Boot ↗</button></div><svg className="orbit-system" viewBox="0 0 800 800" aria-hidden="true">{orbitTerms.map((term, i) => { const r = 92 + i * 35; return <g key={term} className="orbit-ring" style={{ animationDuration: `${22 + i * 4}s, ${4 + i * .27}s`, animationDirection: i % 2 ? 'reverse, normal' : 'normal, normal', transformOrigin: '400px 400px' }}><defs><path id={`${id}-orbit-${i}`} d={`M400,${400 - r} a${r},${r} 0 1,1 0,${2 * r} a${r},${r} 0 1,1 0,-${2 * r}`}/></defs><circle cx="400" cy="400" r={r} fill="none" stroke="currentColor" opacity=".09"/><text fontSize={10 + i * .8} letterSpacing="3" fill="currentColor"><textPath href={`#${id}-orbit-${i}`} textLength={2 * Math.PI * r} lengthAdjust="spacing">{`${term} · `.repeat(5 + i)}</textPath></text></g>; })}</svg><div className="boot-count"><span className="mono">INITIALIZING</span><output aria-live="off" data-testid="boot-count">{count}<sup>%</sup></output><span className="mono">SEED 0xDEADBEEF</span></div><div className="boot-bottom mono"><span>LINUX x86-64 / USERSPACE SUPERVISION</span><span>0{Math.min(8, Math.floor(count / 12.5))} / 08 SYSTEMS</span></div></div>;
}
