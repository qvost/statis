'use client';
import { createContext, useContext, useState, useEffect, useRef, useCallback, type ReactNode } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { EffectsProvider, useEffects, RouteFrame } from '@/generated/motion';
import { modes, type TransitionMode } from '@/lib/effects-types';
import { parseSeed } from '@/lib/determinism';
import { OrbitTextPreloader } from './OrbitTextPreloader';
import { HalftoneSceneFooter } from './HalftoneSceneFooter';
const SeedContext = createContext({ seed: 0xdeadbeefn, setSeed: (_seed: bigint) => { void _seed; } });
export const useSeed = () => useContext(SeedContext);
export function PortalLink({ href, children, className, ariaLabel, transitionMode, ariaCurrent }: {
    href: string;
    children: ReactNode;
    className?: string;
    ariaLabel?: string;
    ariaCurrent?: 'page';
    transitionMode?: TransitionMode;
}) {
    const effects = useEffects();
    return <Link href={href} className={className} aria-label={ariaLabel} aria-current={ariaCurrent} data-transition={transitionMode} onClick={e => { if (!e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey && e.button === 0 && href.startsWith('/') && !href.includes('#')) {
        effects.navigate(href, e, transitionMode);
        e.preventDefault();
    } }}>{children}</Link>;
}
function Header({ replay }: {
    replay: () => void;
}) {
    const path = usePathname(), header = useRef<HTMLElement>(null), drawer = useRef<HTMLDialogElement>(null), { mode, setMode, renderer } = useEffects(), { seed, setSeed } = useSeed();
    const [draft, setDraft] = useState('0xDEADBEEF');
    useEffect(() => {
        let frame = 0;
        const update = () => {
            if (frame) return;
            frame = requestAnimationFrame(() => {
                frame = 0;
                // Every main section shares one paper surface; only the footer is ink.
                // One geometry read replaces a full section scan on every scroll event.
                const footer = document.querySelector<HTMLElement>('.htsf-root');
                const bounds = footer?.getBoundingClientRect();
                const light = String(!(bounds && bounds.top <= 90 && bounds.bottom > 90));
                const scrolled = String((document.querySelector('.slp-route-outlet')?.scrollTop ?? window.scrollY) > 20);
                if (header.current) {
                    if (header.current.dataset.light !== light) header.current.dataset.light = light;
                    if (header.current.dataset.scrolled !== scrolled) header.current.dataset.scrolled = scrolled;
                }
            });
        };
        update();
        window.addEventListener('scroll', update, { passive: true, capture: true });
        window.addEventListener('resize', update);
        return () => {
            cancelAnimationFrame(frame);
            window.removeEventListener('scroll', update, true); window.removeEventListener('resize', update);
        };
    }, [path]);
    useEffect(() => { drawer.current?.close(); }, [path]);
    return <><header ref={header} className="slp-header" data-light="true" data-scrolled="false">
  <PortalLink href="/" className="brand" ariaLabel="Statis overview"><span className="brand-symbol" aria-hidden="true">▥</span>statis<span className="brand-period">.</span></PortalLink>
  <nav aria-label="Main navigation" className="nav-pill">{['Overview', 'Architecture', 'Benchmarks', 'Fixtures', 'Paper'].map((label, i) => { const href = i ? '/' + label.toLowerCase() : '/'; return <PortalLink href={href} key={label} ariaCurrent={path === href ? 'page' : undefined} transitionMode={label === 'Paper' ? 'ZoomBlur' : undefined} className={path === href ? 'nav-item active' : 'nav-item'}><span className="nav-reveal"><span>{label}</span><span aria-hidden="true">{label}</span></span></PortalLink>; })}</nav>
  <div className="header-actions"><PortalLink href="/paper" className="paper-cta" transitionMode="ZoomBlur">Read Paper ↗</PortalLink><a className="github-cta" href="https://github.com/qvost/statis" target="_blank" rel="noreferrer">GitHub ↗</a><button className="settings-trigger" onClick={() => drawer.current?.showModal()} aria-label="Open display controls">⌘</button></div>
 </header>
 <div className="boot-replay"><button onClick={replay} aria-label="Replay Boot Sequence">↺ <span>Replay Boot Sequence</span></button></div>
 <dialog ref={drawer} className="settings-drawer" aria-labelledby="display-title" onClick={e => { if (e.target === e.currentTarget)
        drawer.current?.close(); }}><div className="drawer-inner"><div className="section-kicker"><span>PORTAL / CONTROLS</span><button onClick={() => drawer.current?.close()} aria-label="Close display controls">Close ×</button></div><h2 id="display-title">Same seed.<br />Less motion.</h2><p className="small">TRANSITION RENDERER / {renderer}</p><label htmlFor="transition-mode">Transition Mode</label><select id="transition-mode" value={mode} onChange={e => setMode(e.target.value as TransitionMode)}>{modes.map(m => <option key={m}>{m}</option>)}</select><label htmlFor="visual-seed">Visual seed (unsigned 64-bit)</label><div className="seed-input"><input id="visual-seed" value={draft} onChange={e => setDraft(e.target.value)} aria-invalid={parseSeed(draft) === null}/><button disabled={parseSeed(draft) === null} onClick={() => setSeed(parseSeed(draft)!)}>Apply ↗</button></div><p className="mono small">ACTIVE / 0x{seed.toString(16).toUpperCase()}</p><p>The overview uses clean section boundaries. Seeded dissolves remain on the detail pages; decorative effects are optional.</p><button className="button dark" onClick={() => { drawer.current?.close(); replay(); }}>Replay Boot Sequence ↺</button><p className="small">Motion follows your system’s reduced-motion preference. Content remains available when graphics are unsupported.</p></div></dialog></>;
}
function Interior({ children }: {
    children: ReactNode;
}) {
    const [boot, setBoot] = useState(true), [bootKey, setBootKey] = useState(0);
    const { bootTransition } = useEffects();
    const completing = useRef(false);
    const finish = useCallback(() => { if (completing.current)
        return; completing.current = true; const reveal = () => setBoot(false); const safety = setTimeout(reveal, 1100); Promise.resolve(bootTransition(reveal)).catch(reveal).finally(() => { reveal(); clearTimeout(safety); }); }, [bootTransition]);
    const replay = () => { if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
        setBoot(false);
        return;
    } completing.current = false; setBootKey(k => k + 1); setBoot(true); };
    return <><Header replay={replay}/><RouteFrame><main id="main-content" style={{ opacity: 1, visibility: 'visible' }}>{children}</main><HalftoneSceneFooter /></RouteFrame>{boot && <OrbitTextPreloader key={bootKey} onComplete={finish}/>}</>;
}
export function PortalShell({ children }: {
    children: ReactNode;
}) { const [seed, setSeed] = useState(0xdeadbeefn); return <EffectsProvider><SeedContext.Provider value={{ seed, setSeed }}><Interior>{children}</Interior></SeedContext.Provider></EffectsProvider>; }
