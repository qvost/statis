import {chromium} from 'playwright';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
const base=process.env.PORTAL_URL||'http://127.0.0.1:3000';
const routes=['/','/architecture','/benchmarks','/fixtures','/paper'];
const output=new URL('../test-results/',import.meta.url);await mkdir(output,{recursive:true});
const report={base,checks:[],errors:[],warnings:[],requests:[],status:'running'};
let browser;
try{
 browser=await chromium.launch({headless:true});
 for(const viewport of [{width:1440,height:900},{width:390,height:844}]){
  const context=await browser.newContext({viewport});const page=await context.newPage();
  page.on('console',m=>{if(m.type()==='error')report.errors.push(m.text());if(m.type()==='warning')report.warnings.push(m.text());});page.on('pageerror',e=>report.errors.push(e.message));page.on('response',r=>{if(r.status()>=400)report.requests.push(`${r.status()} ${r.url()}`);});
  await page.addInitScript(()=>{
   const fill=CanvasRenderingContext2D.prototype.fillRect;
   CanvasRenderingContext2D.prototype.fillRect=function(x,y,w,h){
    if(this.canvas.classList.contains('slp-hero-field')&&w===6&&h===6){window.__heroCenter={x,y};window.__heroDraws=(window.__heroDraws||0)+1;}
    return fill.call(this,x,y,w,h);
   };
  });
  await page.goto(base,{waitUntil:'networkidle'});
  const samples=[await page.getByTestId('boot-count').textContent()];await page.waitForTimeout(650);samples.push(await page.getByTestId('boot-count').textContent());await page.waitForTimeout(2500);assert.equal(await page.locator('.orbit-preloader').count(),0);assert.ok(Number(samples[1].replace('%',''))>Number(samples[0].replace('%','')));report.checks.push(`Boot advances and reveals / ${viewport.width}`);assert.equal(await page.locator('.pixel-band').count(),0,'No pixel dissolve bands on the overview');
  if(viewport.width===1440){
   const heading=page.locator('.hero-statis');const bounds=await heading.boundingBox();
   await page.mouse.move(bounds.x+30,bounds.y+25);await page.waitForTimeout(500);
   const before=await page.evaluate(()=>window.__heroCenter);
   await page.mouse.move(bounds.x+130,bounds.y+25);await page.waitForTimeout(500);
   const after=await page.evaluate(()=>window.__heroCenter);
   assert.ok(after.x-before.x>.3,'Hero pointer tracking must continue over foreground text');
   assert.ok(after.x-before.x<10,'Hero displacement must stay bounded and smooth');
   const finalBounds=await heading.boundingBox();assert.equal(finalBounds.x,bounds.x);assert.equal(finalBounds.y,bounds.y);
   await page.waitForTimeout(1200);
   const draws=await page.evaluate(()=>window.__heroDraws);
   await page.waitForTimeout(300);
   assert.equal(await page.evaluate(()=>window.__heroDraws),draws,'Hero stops repainting once the pointer settles');
   report.checks.push('Hero tracks over text, keeps text stationary, and stops idle repainting');
  }
  const theme=await page.locator('.slp-hero').evaluate(e=>getComputedStyle(e).backgroundColor);
  await page.getByRole('button',{name:'Replay Boot Sequence',exact:false}).first().click();await page.getByRole('button',{name:'Skip Boot'}).click();await page.waitForTimeout(1200);assert.equal(await page.locator('.orbit-preloader').count(),0);
  for(const route of routes){if(route!=='/'){await page.locator('.nav-pill').getByRole('link',{name:route==='/paper'?'Paper':route.slice(1)[0].toUpperCase()+route.slice(2),exact:true}).click();await page.waitForURL(base+route);await page.waitForTimeout(1200);}const main=page.locator('#main-content');assert.equal(await main.locator('section').first().evaluate(e=>getComputedStyle(e).backgroundColor),theme,'Shared paper theme across routes');assert.equal(await main.evaluate(e=>getComputedStyle(e).opacity),'1');assert.equal(await main.evaluate(e=>getComputedStyle(e).visibility),'visible');assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`overflow ${route} ${viewport.width}`);for(const url of ['https://github.com/qvost','https://linkedin.com/in/qvost','https://x.com/qvostlost','mailto:qvostwon@gmail.com'])assert.ok(await page.locator(`footer a[href="${url}"]`).count());report.checks.push(`${route} navigation, visible DOM, no horizontal overflow, footer URLs / ${viewport.width}`);}
  for(const asset of ['/benchmarks_svg/time_warp_speedup.svg','/benchmarks_svg/trace_size_comparison.svg','/benchmarks_svg/determinism_matrix.svg','/paper/paper.pdf','/paper/statis_research_paper.pdf','/statis-mark.svg']){const r=await context.request.get(base+asset);assert.equal(r.status(),200);assert.ok((await r.body()).length>(asset.endsWith('.svg')&&asset.includes('statis-mark')?100:1000));report.checks.push(`200 nonempty ${asset}`);}
  const pdf=page.locator('.pdf-viewer');assert.ok((await pdf.boundingBox()).height>0);
  assert.ok(await page.locator('.paper-reader-section').first().evaluate(e=>Boolean(e.compareDocumentPosition(document.querySelector('#paper-pdf'))&Node.DOCUMENT_POSITION_FOLLOWING)),'Web reader precedes PDF');
  await page.getByRole('link',{name:'Jump to PDF & download'}).click();await page.waitForTimeout(700);assert.ok((await page.locator('#paper-pdf').boundingBox()).y<viewport.height);
  await page.goto(base+'/benchmarks');await page.waitForTimeout(3000);for(const name of ['Time-warp speedup','Trace storage','Determinism matrix']){await page.getByRole('button',{name:new RegExp(name,'i')}).first().click();const button=page.getByRole('button',{name:new RegExp(`^Inspect ${name} at full resolution$`,'i')});await button.scrollIntoViewIfNeeded();const image=button.locator('img');assert.ok((await image.boundingBox()).width>0);await button.click();await page.getByRole('slider',{name:'Chart zoom'}).evaluate(e=>{e.value='150';e.dispatchEvent(new Event('input',{bubbles:true}));});await page.getByRole('button',{name:'Close chart inspection'}).click();}
  await page.goto(base);await page.waitForTimeout(3000);const white=page.locator('.stat-section');await white.scrollIntoViewIfNeeded();await page.waitForTimeout(700);assert.equal(await page.locator('.slp-header').getAttribute('data-light'),'true');const liquid=page.getByRole('checkbox',{name:'Enable liquid effect'});assert.equal(await liquid.isChecked(),false);await page.locator('.liquid-stat-grid').hover();assert.equal(await liquid.isChecked(),false);
  const black=page.locator('.crt-section');await black.scrollIntoViewIfNeeded();await page.waitForTimeout(700);assert.equal(await page.locator('.slp-header').getAttribute('data-light'),'true');const selected=page.locator('.target-chips button[aria-pressed=true]');const initialTarget=await selected.textContent();const candidate=page.locator('.target-chips button').filter({hasText:'test_concurrency'});await candidate.hover();assert.equal(await selected.textContent(),initialTarget,'Hover does not swap CRT targets');await candidate.click();assert.equal(await candidate.getAttribute('aria-pressed'),'true');const wave=page.locator('.halftone-wave-hero');await wave.scrollIntoViewIfNeeded();assert.ok((await wave.locator('canvas').boundingBox()).height>200);await page.getByRole('button',{name:'Pause halftone wave'}).click();assert.equal(await page.getByRole('button',{name:'Play halftone wave'}).getAttribute('aria-pressed'),'true');await page.screenshot({path:new URL(`desktop-${viewport.width}.png`,output).pathname,fullPage:true});
  await context.close();
 }
 const reduced=await browser.newContext({reducedMotion:'reduce'});const rp=await reduced.newPage();await rp.addInitScript(()=>{const get=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(type,...args){return type==='webgl'||type==='webgl2'?null:get.call(this,type,...args);};});await rp.goto(base);await rp.waitForTimeout(200);assert.equal(await rp.locator('.orbit-preloader').count(),0);assert.equal(await rp.locator('#main-content').evaluate(e=>getComputedStyle(e).visibility),'visible');await reduced.close();report.checks.push('Reduced motion + no WebGL bypass keeps main visible');
 assert.deepEqual(report.errors,[]);assert.deepEqual(report.requests,[]);assert.deepEqual(report.warnings,[]);report.status='passed';
}catch(error){report.status='blocked_or_failed';report.failure=error.message;console.error(error.message);process.exitCode=1;}finally{await browser?.close();await writeFile(new URL('browser-report.json',output),JSON.stringify(report,null,2)+'\n');console.log(`Browser verification: ${report.status}; ${report.checks.length} checks.`);}
