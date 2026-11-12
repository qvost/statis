import type { ReactNode, MouseEvent } from 'react';
export const modes = ['PixelScan', 'GlitchSlide', 'Tile', 'Swipe', 'Crossfade', 'LensDistortion', 'ZoomBlur'] as const;
export type TransitionMode = typeof modes[number];
export type Effects = {
    mode: TransitionMode;
    setMode: (m: TransitionMode) => void;
    navigate: (href: string, event?: MouseEvent<HTMLAnchorElement>, modeOverride?: TransitionMode) => void;
    bootTransition: (update: () => void) => Promise<void>;
    renderer: 'VFX-JS' | 'native fallback';
};
export type ProviderProps = {
    children: ReactNode;
};
