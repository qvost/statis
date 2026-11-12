'use client';
import { useEffect, useRef, useState } from 'react';
import { createCRT } from '@/generated/crt';
import type { CRTHandle } from '@/lib/crt-fallback';
import { research, fixtureNotes } from '@/lib/research';
import { PortalLink } from './PortalShell';
export function CRTDisplay({ initialTarget = 'test_timewarp', animate = true, showLabels = true }: {
    initialTarget?: string;
    animate?: boolean;
    showLabels?: boolean;
}) {
    const [target, setTarget] = useState(initialTarget), [view, setView] = useState<'stdout' | 'rules' | 'telemetry'>('stdout');
    const host = useRef<HTMLDivElement>(null), monitor = useRef<CRTHandle | null>(null);
    const note = fixtureNotes[target];
    const captured = research.benchmarks.workloads[target as keyof typeof research.benchmarks.workloads]?.native.attempts[0]?.stdout;
    const content = view === 'stdout' ? `${captured ? 'SAVED NATIVE STDOUT' : 'OUTPUT SCHEMA / NOT A RUN CAPTURE'}\n${target}\n─────────────────────────────────\n${captured?.trim() || note.stdout}` : view === 'rules' ? `INTERCEPTION MAP / ${target}\n─────────────────────────────────\n${note.rules.split(' · ').join('\n')}\n\nCLI / src/main.rs\nstatis run --seed 3735928559\n  --quantum 10000 -- ./target/release/${target}` : `SUPERVISED TELEMETRY / UNAVAILABLE\n─────────────────────────────────\nVirtual time        unavailable\nElapsed             unavailable\nTrapped syscalls    unavailable\nRDTSC traps         unavailable\nContext switches    unavailable\n\nLaunch failed before target exec.\nTarget stdout / stderr summary are separate.`;
    const current = useRef(content);
    current.current = content;
    useEffect(() => { const el = host.current!; const observer = new IntersectionObserver(([entry]) => { if (!entry.isIntersecting || monitor.current)
        return; try {
        monitor.current = createCRT(el, current.current, !animate || matchMedia('(prefers-reduced-motion: reduce)').matches);
    }
    catch { /* The accessible text remains the authoritative display. */ } }, { rootMargin: '180px' }); observer.observe(el); return () => { observer.disconnect(); monitor.current?.dispose(); monitor.current = null; }; }, [animate]);
    useEffect(() => { monitor.current?.update(content); }, [content]);
    return <section className="crt-section section" data-surface="black">{showLabels && <div className="section-kicker"><span>02 / THE RUNTIME, UNDER OBSERVATION</span><span>08 FIXTURE BINARIES</span></div>}<div className="crt-layout"><div className="crt-copy"><h2>Unmodified binary.<br /><span className="muted">Inspectable boundary.</span></h2><p>Follow the actual fixture programs through the inputs Statis controls. Every output distinguishes a saved capture from a source-defined schema.</p><div className="target-chips" aria-label="Select target binary">{research.fixtures.map(f => <button key={f.name} aria-pressed={target === f.name} onClick={() => setTarget(f.name)}>{f.name}</button>)}</div><PortalLink href="/fixtures" className="text-link">Explore the fixture atlas ↗</PortalLink></div><div><div className="monitor"><div className="monitor-screen"><div ref={host} className="crt-canvas-host"/><pre className="sr-only">{content}</pre></div><div className="monitor-bezel"><span>STATIS / x86-64</span><span className="monitor-led">● LIVE SCHEMA</span><i /><i /></div></div><div className="crt-tabs" role="group" aria-label="Monitor content">{(['stdout', 'rules', 'telemetry'] as const).map(v => <button key={v} aria-pressed={view === v} onClick={() => setView(v)}>{v === 'stdout' ? 'Target stdout' : v === 'rules' ? 'Interception rules' : 'Summary / stderr'}</button>)}</div><details className="crt-transcript"><summary>Read display as text ↓</summary><pre>{content}</pre></details></div></div></section>;
}
