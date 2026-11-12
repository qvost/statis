import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,stat} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const portal=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const research=JSON.parse(await readFile(path.join(portal,'src/generated/research.json'),'utf8'));
test('All synchronized artifacts are byte-faithful to their SHA-256 manifest',async()=>{for(const [name,hash]of Object.entries(research.hashes)){const local=name.startsWith('author-supplied/')?'paper/'+name.split('/')[1]:name.endsWith('.rs')||['README.md','Cargo.toml','LICENSE'].includes(name)?'source/'+name:name;const bytes=await readFile(path.join(portal,'public',local));assert.equal(createHash('sha256').update(bytes).digest('hex'),hash,name);}});
test('All SVGs have nonzero intrinsic dimensions and the supplied PDFs are intact',async()=>{for(const name of ['time_warp_speedup.svg','trace_size_comparison.svg','determinism_matrix.svg']){const svg=await readFile(path.join(portal,'public/benchmarks_svg',name),'utf8');assert.match(svg,/<svg/);assert.match(svg,/<title/);assert.ok(Number(svg.match(/width="(\d+)"/)[1])>0);assert.ok(Number(svg.match(/height="(\d+)"/)[1])>0);}for(const name of ['paper.pdf','statis_research_paper.pdf']){const pdf=await readFile(path.join(portal,'public/paper',name));assert.equal(pdf.subarray(0,5).toString(),'%PDF-');assert.ok(pdf.length>100000);}});
test('Five built routes resolve every local link and resource to a generated route or asset',async()=>{const routes=['/','/architecture','/benchmarks','/fixtures','/paper'];for(const route of routes){const html=await readFile(path.join(portal,'.next/server/app',route==='/'?'index.html':route.slice(1)+'.html'),'utf8');assert.match(html,/<h1/);if(route==='/'){
 assert.ok(!html.includes('class="pixel-band'),'Removed dissolve bands must not return to the overview');
 assert.ok(!html.includes('RESEARCH PORTAL / v0.1.0'),'Overview has no version stamp');
 assert.ok(!html.includes('RESEARCH NOTES ↓'),'Overview has no decorative research counter');
 assert.ok(!html.includes('01 / RESEARCH IN NUMBERS'),'Overview omits decorative section labels');
}
if(route==='/paper'){
 const reader=html.indexOf('class="paper-reader-section');
 const archive=html.indexOf('class="archive-report');
 const pdf=html.indexOf('id="paper-pdf"');
 assert.ok(reader>=0&&archive>reader&&pdf>archive,'Web reader and archive precede the final PDF section');
 assert.ok(html.includes('loading="lazy"'),'PDF loading is deferred');
}
assert.match(html,/opacity:1;visibility:visible/);assert.ok(!html.includes('BLANK'));for(const m of html.matchAll(/(?:href|src|data)="(\/[^"<>]+)"/g)){const url=m[1].split(/[?#]/)[0].replaceAll('&amp;','&');if(routes.includes(url)||url==='/_not-found'||url.startsWith('/_next/image'))continue;const file=url.startsWith('/_next/static/')?path.join(portal,'.next',url.slice('/_next/'.length)):path.join(portal,'public',url);assert.ok((await stat(file)).isFile(),`${route} → ${url}`);}for(const url of ['https://github.com/qvost','https://linkedin.com/in/qvost','https://x.com/qvostlost','mailto:qvostwon@gmail.com'])assert.ok(html.includes(`href="${url}"`),`${route}: missing ${url}`);}});
test('Saved benchmark numbers preserve incomplete supervised results and exact native lock arrays',()=>{assert.equal(research.benchmarks.status,'incomplete');const w=research.benchmarks.workloads;assert.equal(w.test_timewarp.native.median_seconds,10.000907011);assert.equal(w.test_epoll.native.median_seconds,15.016036295);assert.equal(w.test_concurrency.native.median_seconds,.000848676);for(const work of Object.values(w)){assert.equal(work.statis.median_seconds,null);assert.equal(work.speedup,null);}assert.ok(w.test_concurrency.native.attempts[2].stdout.includes('[0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3]'));});
