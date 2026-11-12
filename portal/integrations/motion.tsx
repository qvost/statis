'use client';
// The full integration is selected only when the real registry packages exist.
// API reference: github.com/fand/vfx-js/blob/main/docs/TRANSITION.md
import { createContext, useContext, useState, useEffect, useCallback, type MouseEvent } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { VFXTransition } from '@vfx-js/react/next';
import { TransitionIn, transition, useVFXTransition } from '@vfx-js/react';
import { PixelScan, GlitchSlide, Tile, Swipe, Crossfade, LensDistortion, ZoomBlur } from '@vfx-js/transitions';
import type { RunOpts } from '@vfx-js/core';
import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { useGSAP } from '@gsap/react';
import Lenis from 'lenis';
import type { Effects, ProviderProps, TransitionMode } from '../src/lib/effects-types';
const presets = { PixelScan, GlitchSlide, Tile, Swipe, Crossfade, LensDistortion, ZoomBlur };
function spec(mode: TransitionMode): RunOpts {
    if (mode === 'ZoomBlur')
        return { transition: ZoomBlur, duration: .65 };
    if (mode === 'PixelScan')
        return { transition: PixelScan, params: { direction: 'center' }, duration: .85 };
    if (mode === 'Tile')
        return { transition: Tile, params: { direction: 'center', shape: 'square' }, duration: .75 };
    if (mode === 'Swipe')
        return { transition: Swipe, params: { direction: 'up' }, duration: .65 };
    return { transition: presets[mode], duration: .7 };
}
const Context = createContext<(Effects & {
    enabled: boolean;
}) | null>(null);
gsap.registerPlugin(ScrollTrigger, useGSAP);
export function EffectsProvider({ children }: ProviderProps) {
    const [mode, setMode] = useState<TransitionMode>('Crossfade'), [enabled, setEnabled] = useState(false), router = useRouter(), pathname = usePathname();
    const { ref, run } = useVFXTransition({ transition: PixelScan, params: { direction: 'center' }, duration: .85 });
    useEffect(() => { const media = matchMedia('(prefers-reduced-motion: reduce)'); const update = () => { if (media.matches) {
        setEnabled(false);
        return;
    } try {
        const probe = document.createElement('canvas');
        const gl = probe.getContext('webgl2');
        setEnabled(Boolean(gl));
        gl?.getExtension('WEBGL_lose_context')?.loseContext();
    }
    catch {
        setEnabled(false);
    } }; update(); media.addEventListener('change', update); return () => media.removeEventListener('change', update); }, []);
    useGSAP(() => {
        if (matchMedia('(prefers-reduced-motion: reduce)').matches)
            return;
        const wrapper = document.querySelector<HTMLElement>('.slp-route-outlet'), content = document.querySelector<HTMLElement>('.slp-route-content');
        const lenis = wrapper && content ? new Lenis({ wrapper, content, lerp: .09, smoothWheel: true, autoRaf: false }) : new Lenis({ lerp: .09, smoothWheel: true, autoRaf: false });
        if (wrapper)
            ScrollTrigger.defaults({ scroller: wrapper });
        const tick = (seconds: number) => lenis.raf(seconds * 1000);
        lenis.on('scroll', ScrollTrigger.update);
        gsap.ticker.add(tick);
        gsap.ticker.lagSmoothing(0);
        ScrollTrigger.refresh();
        return () => { gsap.ticker.remove(tick); lenis.off('scroll', ScrollTrigger.update); lenis.destroy(); ScrollTrigger.defaults({ scroller: window }); };
    }, { dependencies: [enabled] });
    useEffect(() => { const frame = requestAnimationFrame(() => ScrollTrigger.refresh()); return () => cancelAnimationFrame(frame); }, [pathname]);
    const bootTransition = useCallback(async (update: () => void) => { if (!enabled) {
        update();
        return;
    } try {
        await run(update, spec(mode));
    }
    catch {
        update();
    } }, [enabled, mode, run]);
    const navigate = (href: string, event?: MouseEvent<HTMLAnchorElement>, modeOverride?: TransitionMode) => { if (enabled && event)
        transition(spec(modeOverride ?? mode))(event); router.push(href); };
    return <Context.Provider value={{ mode, setMode, bootTransition, navigate, enabled, renderer: enabled ? 'VFX-JS' : 'native fallback' }}><div ref={ref} className="slp-boot-transition-target">{children}</div></Context.Provider>;
}
export function useEffects() { const c = useContext(Context); if (!c)
    throw new Error('EffectsProvider missing'); return c; }
export function RouteFrame({ children }: ProviderProps) { const { mode, enabled } = useEffects(); return enabled ? <VFXTransition {...spec(mode)} autoReverse className="slp-route-outlet" style={{ overflowY: 'auto', overflowX: 'hidden' }}><div className="slp-route-content">{children}</div></VFXTransition> : <>{children}</>; }
export function RouteEntrance() { const { mode, enabled } = useEffects(); return enabled ? <TransitionIn {...spec(mode)}/> : null; }
