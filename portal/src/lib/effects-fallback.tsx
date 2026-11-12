'use client';

import { createContext, useContext, useState, useEffect, useCallback, useRef } from 'react';
import { useRouter, usePathname } from 'next/navigation';
import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import type { Effects, ProviderProps, TransitionMode } from './effects-types';

const Context = createContext<Effects | null>(null);

function reveal(mode: TransitionMode) {
    const content = document.querySelector<HTMLElement>('#main-content');
    // Blur only the incoming introduction, not the entire long document.
    const main = mode === 'ZoomBlur' ? content?.querySelector<HTMLElement>('section') ?? content : content;
    if (!main || !main.animate || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    // Native recovery approximation. The installed adapter uses the actual VFX-JS shader.
    const frames = mode === 'ZoomBlur'
        ? [{ opacity: .35, filter: 'blur(8px)', transform: 'scale(1.035)' },
           { opacity: 1, filter: 'blur(0px)', transform: 'scale(1)' }]
        : [{ opacity: .6 }, { opacity: 1 }];
    return main.animate(frames, { duration: mode === 'ZoomBlur' ? 480 : 240, easing: 'cubic-bezier(.16,1,.3,1)' });
}

export function EffectsProvider({ children }: ProviderProps) {
    const router = useRouter(), path = usePathname();
    const [mode, setMode] = useState<TransitionMode>('Crossfade');
    const pending = useRef<TransitionMode | null>(null);
    const currentMode = useRef(mode);
    currentMode.current = mode;
    useEffect(() => {
        gsap.registerPlugin(ScrollTrigger);
        const update = () => ScrollTrigger.update();
        window.addEventListener('scroll', update, { passive: true });
        return () => window.removeEventListener('scroll', update);
    }, []);
    useEffect(() => {
        const selected = pending.current;
        pending.current = null;
        let animation: Animation | undefined;
        const id = requestAnimationFrame(() => {
            ScrollTrigger.refresh();
            if (selected) animation = reveal(selected);
        });
        return () => { cancelAnimationFrame(id); animation?.cancel(); };
    }, [path]);
    const bootTransition = useCallback(async (update: () => void) => { update(); }, []);
    return <Context.Provider value={{
        mode, setMode, bootTransition, renderer: 'native fallback',
        navigate: (href, _event, modeOverride) => {
            pending.current = modeOverride ?? currentMode.current;
            router.push(href);
        },
    }}>{children}</Context.Provider>;
}
export function useEffects() {
    const context = useContext(Context);
    if (!context) throw new Error('EffectsProvider missing');
    return context;
}
export function RouteEntrance() { return null; }
export function RouteFrame({ children }: ProviderProps) { return <>{children}</>; }
