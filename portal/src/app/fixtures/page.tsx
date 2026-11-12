import type { Metadata } from 'next';
import { PageIntro } from '@/components/PageIntro';
import { PixelDissolve } from '@/components/ui-system/PixelDissolve';
import { MinimapParallaxScroll } from '@/components/ui-system/FixtureAtlas';
import { CRTDisplay } from '@/components/ui-system/CRTDisplay';
export const metadata: Metadata = { title: 'Fixtures & CLI' };
export default function Fixtures() { return <><PageIntro title="A small program. A precise question." description="Eight workspace fixtures probe time, entropy, threads, timestamps, faults, event waits, replay, and process identity. Inspect the source and compose an exact CLI invocation."/><PixelDissolve from="blue" to="white"/><MinimapParallaxScroll /><PixelDissolve from="white" to="black"/><CRTDisplay initialTarget="test_record_replay"/></>; }
