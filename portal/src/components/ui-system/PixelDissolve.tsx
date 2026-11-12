'use client';
import { useMemo, useRef, useEffect, type CSSProperties } from 'react';
import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { seededOrder } from '@/lib/determinism';
import { useSeed } from './PortalShell';
export function PixelDissolve({ from, to }: {
    from: 'blue' | 'black' | 'white';
    to: 'blue' | 'black' | 'white';
}) {
    const { seed } = useSeed(), ref = useRef<HTMLDivElement>(null);
    const order = useMemo(() => { const items = seededOrder(seed); const rank = new Array<number>(150); items.forEach((n, i) => rank[n] = i); return rank; }, [seed]);
    useEffect(() => { if (!ref.current || matchMedia('(prefers-reduced-motion: reduce)').matches)
        return; gsap.registerPlugin(ScrollTrigger); const cells = ref.current.children; const context = gsap.context(() => { gsap.fromTo(cells, { opacity: 1 }, { opacity: 0, stagger: (_, el) => order[Number((el as HTMLElement).dataset.cell)] * .004, ease: 'none', scrollTrigger: { trigger: ref.current, start: 'top 88%', end: 'bottom 35%', scrub: .4 } }); }, ref); return () => context.revert(); }, [order]);
    return <div ref={ref} className={`pixel-band from-${from} to-${to}`} data-surface={to} data-seed={seed.toString()} aria-hidden="true">{order.map((rank, i) => <i key={i} data-cell={i} style={{ '--rank': rank } as CSSProperties}/>)}</div>;
}
