'use client';

import { useEffect, useRef, useState } from 'react';
import { PortalLink } from './PortalShell';

/** Self-contained Canvas interpretation; no remote bitmap or video dependency. */
export function HalftoneWaveHero() {
    const canvas = useRef<HTMLCanvasElement>(null);
    const phaseRef = useRef(0);
    const [paused, setPaused] = useState(false);
    const [reduced, setReduced] = useState(false);

    useEffect(() => {
        const media = matchMedia('(prefers-reduced-motion: reduce)');
        const update = () => setReduced(media.matches);
        update(); media.addEventListener('change', update);
        return () => media.removeEventListener('change', update);
    }, []);

    useEffect(() => {
        const element = canvas.current;
        const ctx = element?.getContext('2d');
        if (!element || !ctx) return;
        let frame = 0, width = 0, height = 0, visible = false, previous = 0;
        const media = matchMedia('(prefers-reduced-motion: reduce)');
        const draw = () => {
            ctx.clearRect(0, 0, width, height);
            if (!width || !height) return;
            const phase = phaseRef.current;
            const columns = Math.min(100, Math.max(40, Math.floor(width / 12)));
            const rows = 24;
            for (let row = 0; row < rows; row++) {
                const v = row / (rows - 1), depth = .36 + v * .64;
                ctx.fillStyle = `rgba(77,94,66,${.12 + depth * .65})`;
                ctx.beginPath();
                for (let column = 0; column < columns; column++) {
                    const u = column / (columns - 1);
                    const wave = Math.sin(u * 10.5 + v * 4.5 - phase) * .55
                        + Math.cos(u * 5.2 - v * 6.4 + phase * .65) * .27;
                    const envelope = Math.pow(Math.sin(u * Math.PI), .7);
                    const x = width * .5 + (u - .5) * width * (1.12 - v * .12);
                    const y = height * (.36 + v * .57) - wave * envelope * height * .19 * depth;
                    const radius = .45 + depth * 1.25 + (wave + .82) * .32;
                    ctx.moveTo(x + radius, y); ctx.arc(x, y, radius, 0, Math.PI * 2);
                }
                ctx.fill();
            }
        };
        const animate = (time: number) => {
            frame = 0;
            if (!visible || paused || media.matches || document.hidden) return;
            if (!previous || time - previous >= 1000 / 20) {
                phaseRef.current += previous ? Math.min(time - previous, 50) * .00025 : 0;
                previous = time; draw();
            }
            frame = requestAnimationFrame(animate);
        };
        const restart = () => {
            cancelAnimationFrame(frame); frame = 0; previous = 0; draw();
            if (visible && !paused && !media.matches && !document.hidden) frame = requestAnimationFrame(animate);
        };
        const resize = new ResizeObserver(() => {
            const rect = element.getBoundingClientRect(), dpr = Math.min(devicePixelRatio || 1, 1.5);
            width = rect.width; height = rect.height;
            element.width = Math.round(width * dpr); element.height = Math.round(height * dpr);
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0); restart();
        });
        const observer = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; restart(); });
        resize.observe(element); observer.observe(element);
        document.addEventListener('visibilitychange', restart); media.addEventListener('change', restart);
        return () => {
            cancelAnimationFrame(frame); resize.disconnect(); observer.disconnect();
            document.removeEventListener('visibilitychange', restart); media.removeEventListener('change', restart);
        };
    }, [paused]);

    return <section className="halftone-wave-hero section" data-surface="white" aria-labelledby="wave-heading">
        <div className="wave-editorial">
            <span className="wave-index" aria-hidden="true">∿</span>
            <h2 id="wave-heading">An idle wait.<br /><em>A different timeline.</em></h2>
            <div className="wave-description"><p>A deadline is a position in virtual time. When tracked work becomes idle, Statis can advance to the next one.</p><PortalLink href="/benchmarks" transitionMode="ZoomBlur" className="text-link">Follow the evidence ↗</PortalLink></div>
        </div>
        <div className="halftone-wave-field"><canvas ref={canvas} aria-hidden="true" /><div className="wave-ground-rule" aria-hidden="true" /></div>
        <div className="wave-caption"><span>A time-field illustration, not a runtime trace.</span><button onClick={() => setPaused(value => !value)} disabled={reduced} aria-pressed={paused} aria-label={paused ? 'Play halftone wave' : 'Pause halftone wave'}>{reduced ? 'Still image · reduced motion' : paused ? 'Play wave ↗' : 'Pause wave Ⅱ'}</button></div>
    </section>;
}
