import type { Metadata } from 'next';
import { PageIntro } from '@/components/PageIntro';
import { PaperReader } from '@/components/PaperReader';

export const metadata: Metadata = { title: 'Paper & Web Reader' };

export default function Paper() {
    return <>
        <PageIntro title="The report. And the record." description="Read the seven-page technical report by Siddid Pandey, alongside the current implementation review. The original PDF and download are available at the end.">
            <a href="#abstract" className="button dark">Start reading ↓</a>
            <a href="#paper-pdf" className="text-link">Jump to PDF & download ↓</a>
        </PageIntro>
        <PaperReader />
        <section className="archive-report section" data-surface="white">
            <details><summary>Earlier five-page report & web reader ↓</summary>
                <p><a href="/paper/paper.pdf" download>Download the earlier PDF ↓</a></p>
                <PaperReader version="repository" />
            </details>
        </section>
        <section id="paper-pdf" className="paper-viewer-section section" data-surface="white" aria-labelledby="pdf-heading">
            <div className="pdf-toolbar">
                <div><h2 id="pdf-heading">The original PDF.</h2><p>Seven pages · Siddid Pandey</p></div>
                <div><a href="/paper/statis_research_paper.pdf" target="_blank" rel="noreferrer" className="button outline">Open PDF ↗</a><a href="/paper/statis_research_paper.pdf" download className="button dark">Download PDF ↓</a></div>
            </div>
            <iframe className="pdf-viewer" src="/paper/statis_research_paper.pdf#view=FitH" loading="lazy" title="Author-supplied seven-page Statis technical report" />
            <a href="#abstract" className="text-link pdf-reader-return">Back to the web reader ↑</a>
        </section>
    </>;
}
