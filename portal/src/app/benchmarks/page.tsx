import type { Metadata } from 'next';
import { PageIntro } from '@/components/PageIntro';
import { PixelDissolve } from '@/components/ui-system/PixelDissolve';
import { BenchmarkRecord, AccordionFrames, TraceInspector } from '@/components/ui-system/BenchmarkSections';
import { LiquidStatGrid } from '@/components/ui-system/LiquidStatGrid';
export const metadata: Metadata = { title: 'Benchmarks' };
export default function Benchmarks() { return <><PageIntro title="Incomplete is a result." description="Native baselines, supervised launch diagnostics, and supplied reference scenarios. The provenance travels with every number."/><PixelDissolve from="blue" to="white"/><BenchmarkRecord /><LiquidStatGrid /><AccordionFrames /><PixelDissolve from="white" to="black"/><TraceInspector /></>; }
