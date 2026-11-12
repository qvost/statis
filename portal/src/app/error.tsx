'use client';
export default function ErrorPage({ reset }: {
    reset: () => void;
}) { return <section className="page-intro section" data-surface="blue"><span className="eyebrow">RENDER INTERRUPTED</span><h1>The view could not load.</h1><p>The repository artifacts remain directly accessible.</p><button onClick={reset} className="button light">Try again ↺</button><a href="/docs/benchmark_results.json">Open the benchmark record ↗</a></section>; }
