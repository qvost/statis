import { HalftoneWaveHero } from './ui-system/HalftoneWaveHero';
import { HeroField } from './ui-system/HeroField';
import { PortalLink } from './ui-system/PortalShell';
import { LiquidStatGrid } from './ui-system/LiquidStatGrid';
import { CRTDisplay } from './ui-system/CRTDisplay';
import { SupervisionFlow, SubsystemCarousel, TerminalTextReveal, GridScrambleHover, DrawnPathFeatures } from './ui-system/ArchitectureSections';
import { AccordionFrames, DiagnosticBanner } from './ui-system/BenchmarkSections';
import { RouteEntrance } from '@/generated/motion';

export function SettlementLayerPage() {
    return <div className="overview-page">
        <RouteEntrance />
        <section className="slp-hero slp-on-blue" data-surface="blue">
            <HeroField />
            <div className="hero-main">
                <div><h1><span className="hero-statis">Statis:</span><br />Seed-Deterministic<br />Userspace Supervision<br />for Linux x86-64<span className="cyan">.</span></h1></div>
                <div className="hero-aside">
                    <p className="hero-lead">Some inputs can be controlled.<br />Every claim should be inspected.</p>
                    <p>Virtual time, seeded randomness, and tracked identities for unmodified Linux binaries. A research runtime with its limits in the record.</p>
                    <div className="hero-links"><PortalLink href="/architecture" transitionMode="ZoomBlur">Explore the system ↗</PortalLink><a href="#research">Explore the research ↓</a></div>
                </div>
            </div>
        </section>
        <section className="research-intro section" data-surface="blue" id="research">
            <div className="intro-layout">
                <h2>What if a long wait<br />was just a deadline?</h2>
                <div><p>Statis supervises selected system calls at the binary boundary. When every tracked thread is blocked, a monotonic priority time-wheel can advance to the next deadline.</p><p>That mechanism is implemented. The speed it achieves on a working host is a separate experiment.</p></div>
            </div>
            <div className="lead-highlight">
                <div className="paper-mini" aria-hidden="true"><span>STATIS</span><i /><i /><i /><b>Technical Report</b><i /><i /><div>Δq = 100 ns</div></div>
                <div><h3>A userspace boundary<br />for repeatable experiments.</h3><p>Siddid Pandey · Independent Systems Researcher<br />Seven-page technical report</p></div>
                <div className="lead-links"><PortalLink href="/paper" transitionMode="ZoomBlur">Read the paper ↗</PortalLink><PortalLink href="/benchmarks">Inspect the benchmark record ↗</PortalLink></div>
            </div>
        </section>
        <HalftoneWaveHero />
        <LiquidStatGrid showLabels={false} />
        <CRTDisplay animate={false} showLabels={false} />
        <section className="pipeline-section section" data-surface="black">
            <h2>At the edge of the binary,<br /><span className="muted">a controlled conversation.</span></h2>
            <PortalLink href="/architecture" className="text-link section-detail-link">Architecture in detail ↗</PortalLink>
            <SupervisionFlow />
            <SubsystemCarousel showLabels={false} />
        </section>
        <TerminalTextReveal animate={false} showLabels={false} />
        <DrawnPathFeatures animate={false} showLabels={false} />
        <GridScrambleHover showLabels={false} />
        <AccordionFrames showLabels={false} />
        <section className="record-preview section" data-surface="white">
            <DiagnosticBanner />
            <div className="record-preview-bottom"><p>Experiments that fail stay in the record.</p><PortalLink href="/benchmarks" className="button dark">Inspect native baselines & diagnostics ↗</PortalLink></div>
        </section>
    </div>;
}
