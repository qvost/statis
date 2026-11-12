'use client';

import { useEffect, useRef } from 'react';

export function HeroField() {
    const ref = useRef<HTMLCanvasElement>(null);
    useEffect(() => {
        const canvas = ref.current;
        const hero = canvas?.closest<HTMLElement>('.slp-hero');
        const ctx = canvas?.getContext('2d');
        if (!canvas || !hero || !ctx) return;

        const media = matchMedia('(prefers-reduced-motion: reduce)');
        let frame = 0, width = 0, height = 0, visible = false, lastTime = 0;
        let bounds = hero.getBoundingClientRect();
        const position = { x: 0, y: 0 }, target = { x: 0, y: 0 };
        // Cache the line geometry; idle pages have no animation loop at all.
        let paths: Path2D[] = [];
        const colors = Array.from({ length: 8 }, (_, index) => index === 7
            ? 'rgba(77,94,66,.36)' : `rgba(77,94,66,${.035 + index * .012})`);
        const rebuild = () => {
            paths = Array.from({ length: 8 }, () => new Path2D());
            for (let i = 0; i < 160; i++) {
                const angle = i / 160 * Math.PI * 2;
                const inner = 65 + Math.sin(i * 2.399) * 17;
                const outer = Math.max(width, height) * (.37 + (Math.sin(i * 1.77) + 1) * .13);
                const path = paths[i % 8];
                path.moveTo(Math.cos(angle) * inner, Math.sin(angle) * inner);
                path.lineTo(Math.cos(angle) * outer, Math.sin(angle) * outer);
            }
        };
        const draw = () => {
            if (!width || !height) return;
            ctx.clearRect(0, 0, width, height);
            const x = width * .68 + position.x, y = height * .4 + position.y;
            ctx.save(); ctx.translate(x, y);
            paths.forEach((path, index) => {
                ctx.strokeStyle = colors[index]; ctx.lineWidth = index === 7 ? 1 : .55;
                ctx.stroke(path);
            });
            ctx.strokeStyle = 'rgba(77,94,66,.25)';
            ctx.beginPath(); ctx.arc(0, 0, 126, 0, Math.PI * 2); ctx.stroke(); ctx.restore();
            ctx.fillStyle = '#4d5e42'; ctx.fillRect(x - 3, y - 3, 6, 6);
        };
        const animate = (time: number) => {
            frame = 0;
            if (!visible || document.hidden || media.matches) return;
            const elapsed = lastTime ? time - lastTime : 1000 / 30;
            if (elapsed >= 1000 / 30) {
                const ease = 1 - Math.exp(-Math.min(elapsed, 50) / 100);
                lastTime = time;
                position.x += (target.x - position.x) * ease;
                position.y += (target.y - position.y) * ease;
                if (Math.abs(target.x - position.x) + Math.abs(target.y - position.y) < .02) {
                    position.x = target.x; position.y = target.y; draw(); return;
                }
                draw();
            }
            frame = requestAnimationFrame(animate);
        };
        const schedule = () => {
            if (!frame && visible && !document.hidden && !media.matches) frame = requestAnimationFrame(animate);
        };
        const move = (event: PointerEvent) => {
            if (event.pointerType === 'touch' || media.matches || !bounds.width || !bounds.height) return;
            target.x = Math.max(-15, Math.min(15, ((event.clientX - bounds.left) / bounds.width - .5) * 30));
            target.y = Math.max(-12, Math.min(12, ((event.clientY - bounds.top) / bounds.height - .5) * 24));
            schedule();
        };
        const leave = () => { target.x = 0; target.y = 0; schedule(); };
        const refreshBounds = () => { bounds = hero.getBoundingClientRect(); };
        const restart = () => {
            cancelAnimationFrame(frame); frame = 0; lastTime = 0;
            if (media.matches) { position.x = 0; position.y = 0; target.x = 0; target.y = 0; }
            draw();
            if (position.x !== target.x || position.y !== target.y) schedule();
        };
        const resize = new ResizeObserver(() => {
            const rect = canvas.getBoundingClientRect(), dpr = Math.min(devicePixelRatio || 1, 1.25);
            width = rect.width; height = rect.height; refreshBounds();
            canvas.width = Math.round(width * dpr); canvas.height = Math.round(height * dpr);
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0); rebuild(); restart();
        });
        const intersection = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; refreshBounds(); restart(); });
        resize.observe(canvas); intersection.observe(hero);
        hero.addEventListener('pointermove', move, { passive: true });
        hero.addEventListener('pointerleave', leave);
        window.addEventListener('scroll', refreshBounds, { passive: true, capture: true });
        document.addEventListener('visibilitychange', restart); media.addEventListener('change', restart);
        return () => {
            cancelAnimationFrame(frame); resize.disconnect(); intersection.disconnect();
            hero.removeEventListener('pointermove', move); hero.removeEventListener('pointerleave', leave);
            window.removeEventListener('scroll', refreshBounds, true);
            document.removeEventListener('visibilitychange', restart); media.removeEventListener('change', restart);
        };
    }, []);
    return <canvas ref={ref} className="slp-hero-field" aria-hidden="true" />;
}
