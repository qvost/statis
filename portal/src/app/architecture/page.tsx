import type { Metadata } from 'next';
import { PageIntro } from '@/components/PageIntro';
import { PixelDissolve } from '@/components/ui-system/PixelDissolve';
import { SupervisionFlow, SubsystemCarousel, GridScrambleHover, TerminalTextReveal, DrawnPathFeatures } from '@/components/ui-system/ArchitectureSections';
export const metadata: Metadata = { title: 'Architecture' };
export default function Architecture() { return <><PageIntro title="The boundary is the system." description="Follow a target ELF from launch to trace. Every module here maps to the Rust source; every emulation stops at a stated boundary."/><section className="pipeline-section section" data-surface="blue"><h2>One process. Selected inputs.<br />An explicit control plane.</h2><SupervisionFlow /><SubsystemCarousel /></section><PixelDissolve from="blue" to="white"/><TerminalTextReveal /><DrawnPathFeatures /><PixelDissolve from="white" to="blue"/><GridScrambleHover /><PixelDissolve from="blue" to="black"/></>; }
