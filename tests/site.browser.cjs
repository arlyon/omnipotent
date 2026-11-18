/* Uses an existing Playwright installation, never installs packages/browsers. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_PATH || 'playwright');
const base = process.env.SITE_URL || 'http://127.0.0.1:8000';
const output = process.env.SCREENSHOT_DIR || '/tmp/omnipotent-browser-checks';
fs.mkdirSync(output, { recursive: true });

async function scrollCollection(page, amount) {
  await page.evaluate((progress) => {
    const rect = document.querySelector('#library').getBoundingClientRect();
    const absolute = scrollY + rect.top;
    // Browser scroll positions are quantized. Round forward so the final
    // arrival is reached even when image/layout heights contain fractions.
    scrollTo({ top: Math.ceil(absolute - innerHeight * 0.45 + progress * (rect.height - innerHeight * 0.55)), behavior: 'instant' });
  }, amount);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

const detailCases = [
  {id:'battery-estimate', label:'Battery calculation', title:'A year between charges.', content:[/37\.8 Wh nominal/, /15 to 38 months/, /full power-off between reed-sensor wakes/]},
  {id:'preview-details', label:'About this preview', title:'About this preview', content:[/example collections for the device/, /not a live download/]},
  {id:'medical-details', label:'Sources and editions', title:'Sources and editions', content:[/medical collection is still to be selected/, /publication dates/]},
  {id:'kindle-details', label:'What about Kindle?', title:'What about Kindle?', content:[/Kindle model, file format, and transfer method/, /Direct Kindle transfer/]},
  {id:'build-details', label:'Build details', title:'Build details', content:[/main board and Rust firmware/, /Download the 3D concept/]},
  {id:'format-details', label:'Browse every file type', title:'Every format, in one place.', content:[/complete upstream Xberg registry/, /preparation build/]},
  {id:'transport-details', label:'Protocol details', title:'The right protocol for each job.', content:[/OPDS is a catalogue and acquisition protocol/, /LoRa is a low-bandwidth packet link/]},
];

async function checkDetailsModal(page, name, source) {
  const trigger = page.getByRole('button', { name:source.label, exact:true });
  const dialog = page.locator('#details-dialog');
  await trigger.click();
  await page.getByRole('dialog', { name:source.title, exact:true }).waitFor();
  assert.equal(await page.locator('main').evaluate(element => element.inert), true);
  assert.equal(await page.locator(`#${source.id}`).isVisible(), false);
  for (const pattern of source.content) assert.match(await dialog.textContent(), pattern);
  if (source.id === 'build-details') assert.equal(await dialog.getByRole('link', {name:'Download the 3D concept'}).getAttribute('href'), 'assets/models/omnipotent/omnipotent-1k.glb');
  assert.equal(await page.locator('.modal-close').evaluate(element => element === document.activeElement), true);
  await dialog.evaluate(element => Promise.all(element.getAnimations({subtree:true}).map(animation => animation.finished)));
  await page.screenshot({path:path.join(output, `${name}-${source.id}-modal.png`)});
  for (const key of ['Tab', 'Tab', 'Shift+Tab']) {
    await page.keyboard.press(key);
    assert.equal(await dialog.evaluate(element => element.contains(document.activeElement)), true);
  }
  await page.locator('#assembly-toggle').evaluate(element => element.focus());
  assert.equal(await dialog.evaluate(element => element.contains(document.activeElement)), true);
  const backgroundScroll = await page.evaluate(() => scrollY);
  await page.mouse.wheel(0, 500);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await page.evaluate(() => scrollY), backgroundScroll);
  const fits = await page.locator('.modal-panel').evaluate(element => {
    const rect = element.getBoundingClientRect();
    const body = element.querySelector('.modal-body');
    return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight && body.scrollWidth <= body.clientWidth;
  });
  assert.equal(fits, true, `${name}: modal must fit without horizontal scrolling`);
  await page.locator('.modal-body').evaluate(element => { element.scrollTop = 0; });
  await page.keyboard.press('Escape');
  assert.equal(await dialog.isVisible(), false);
  assert.equal(await page.locator('main').evaluate(element => element.inert), false);
  assert.equal(await trigger.evaluate(element => element === document.activeElement), true);
  await trigger.click();
  await page.getByRole('button', {name:'Close dialog'}).click();
  assert.equal(await dialog.isVisible(), false);
  await trigger.click();
  await page.locator('.modal-backdrop').click({position:{x:4,y:4}});
  assert.equal(await dialog.isVisible(), false);
  assert.equal(await trigger.evaluate(element => element === document.activeElement), true);
  assert.equal(await page.evaluate(() => document.documentElement.classList.contains('modal-open')), false);
}

async function checkCatalogue(page, name) {
  await page.getByRole('button', {name:'Browse every file type', exact:true}).click();
  const dialog = page.getByRole('dialog', {name:'Every format, in one place.', exact:true});
  await dialog.waitFor();
  await page.waitForFunction(() => document.querySelectorAll('#format-catalogue .catalogue-item').length === 478);
  const query = dialog.getByLabel('Find a file type');
  const scope = dialog.getByLabel('Show', {exact:true});
  await query.fill('');
  await scope.selectOption('all');
  assert.equal(await dialog.locator('[data-kind="format"]:visible').count(),107);
  assert.equal(await dialog.locator('[data-kind="code"]:visible').count(),371);
  await query.fill('.DOCX');
  assert.equal(await dialog.locator('.catalogue-item:visible').count(),1);
  assert.match(await dialog.locator('.catalogue-item:visible').textContent(), /Word document/);
  await query.fill('application/x-jsonlines');
  assert.equal(await dialog.locator('.catalogue-item:visible').count(),1);
  assert.match(await dialog.locator('.catalogue-item:visible').textContent(), /NDJSON/);
  await query.fill('text/x-pandoc');
  assert.equal(await dialog.locator('.catalogue-item:visible').count(),1);
  assert.match(await dialog.locator('.catalogue-item:visible').textContent(), /MIME-only format/);
  await scope.selectOption('code');
  await query.fill('Rust');
  assert.equal(await dialog.locator('.catalogue-item:visible').count(),1);
  assert.match(await dialog.locator('[data-catalogue-status]').textContent(), /1 code\/configuration grammar/);
  await query.fill('');
  assert.equal(await dialog.locator('[data-kind="code"]:visible').count(),371);
  await scope.selectOption('formats');
  assert.equal(await dialog.locator('[data-kind="format"]:visible').count(),107);
  const mimeToggle = dialog.getByLabel('Show MIME types');
  await mimeToggle.check();
  assert.equal(await dialog.locator('.catalogue-mime:visible').count(),107);
  await mimeToggle.uncheck();
  assert.equal(await dialog.locator('.catalogue-mime:visible').count(),0);
  await query.fill('<img src=x onerror=alert(1)>');
  assert.equal(await dialog.locator('.catalogue-item:visible').count(),0);
  assert.equal(await dialog.locator('img').count(),0);
  assert.match(await dialog.locator('[data-catalogue-status]').textContent(),/No matching file types/);
  await scope.selectOption('all');
  await query.fill('PDF');
  await page.screenshot({path:path.join(output, `${name}-format-search.png`)});
  await query.fill('');
  await page.keyboard.press('Escape');
}

(async () => {
  const browser = await chromium.launch({ headless: true, chromiumSandbox: true,
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  try {
    const errors = [], failed = [], external = [];
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, deviceScaleFactor: 1 });
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
    page.on('response', (response) => { if (response.status() >= 400) failed.push(response.url()); });
    page.on('request', (request) => {
      if (/^https?:/.test(request.url()) && new URL(request.url()).origin !== new URL(base).origin) external.push(request.url());
    });
    await page.goto(base);
    await page.waitForFunction(() => document.querySelector('#product-viewport').classList.contains('model-ready'));
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.screen === 'blank');
    assert.equal(await page.locator('#model-host').getAttribute('data-seated'), '0.000');
    await page.screenshot({ path: path.join(output, 'front-open.png') });
    await page.getByRole('button', { name: 'Insert base' }).click();
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.screen === 'ready');
    assert.equal(await page.locator('#model-host').getAttribute('data-seated'), '1.000');
    await page.screenshot({ path: path.join(output, 'front-ready.png') });
    await page.getByRole('button', { name: 'Open base' }).click();
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.seated === '0.000');
    console.log('PASS: blank open model, manual seating, ready only after seating, reopening');

    // Reload clears the deliberate manual override, restoring the scroll story.
    await page.reload();
    await page.waitForFunction(() => document.querySelector('#product-viewport').classList.contains('model-ready'));
    await scrollCollection(page, 0);
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.seated === '1.000');
    for (let count = 1; count <= 6; count += 1) {
      await scrollCollection(page, Math.min(1, (count + 0.04) / 6));
      await page.waitForFunction((expected) => document.querySelector('#model-host').dataset.loaded === String(expected), count);
      assert.equal(await page.locator('#collection-count').textContent(), `${count} / 6 collections`);
      assert.match(await page.locator('#collection-display').getAttribute('aria-label'), new RegExp(`${count} of 6`));
      assert.equal(await page.locator('.orbit-bubble[data-landed="true"]').count(), count);
      if (count === 2 || count === 6) await page.screenshot({ path: path.join(output, `collections-${count}.png`) });
    }
    await scrollCollection(page, 2.04 / 6);
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.loaded === '2');
    await page.getByRole('button', { name: 'Pause scroll-linked animation' }).click();
    await scrollCollection(page, 5.04 / 6);
    assert.equal(await page.locator('#model-host').getAttribute('data-loaded'), '2');
    await page.getByRole('button', { name: 'Resume scroll-linked animation' }).click();
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.loaded === '5');
    await page.evaluate(() => scrollTo({ top: 0, behavior: 'instant' }));
    await page.waitForFunction(() => document.querySelector('#model-host').dataset.screen === 'blank');
    console.log('PASS: six arrivals update display one by one, reverse scrolling, pause/resume, return to blank');

    await page.locator('[data-screen="access"]').click();
    await page.waitForFunction(() => document.querySelector('#epaper').getAttribute('aria-label').includes('Wi-Fi online'));
    const component = await page.evaluate(async () => {
      const { createEpaperSurface } = await import('./js/epaper.mjs');
      const el = document.querySelector('#epaper');
      const canvas = el.canvas;
      const pixels = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      const colors = new Set();
      for (let i = 0; i < pixels.length; i += 4) colors.add(`${pixels[i]},${pixels[i+1]},${pixels[i+2]},${pixels[i+3]}`);
      const textPixels = canvas.getContext('2d').getImageData(92, 34, 188, 66).data;
      let monochromeText = true;
      for (let i = 0; i < textPixels.length; i += 4) {
        const rgb = `${textPixels[i]},${textPixels[i+1]},${textPixels[i+2]}`;
        if (rgb !== '245,243,233' && rgb !== '25,25,24') monochromeText = false;
      }
      const first = document.createElement('epaper-display'), second = document.createElement('epaper-display');
      first.state = 'ready'; second.state = 'sleep';
      document.body.append(first, second);
      const unchanged = second.canvas.toDataURL();
      first.content = { title: '<img src=x onerror=alert(1)>', detail: 'A long example that must remain within the canvas', icon: 'books' };
      const independent = unchanged === second.canvas.toDataURL();
      let invalidRejected = false;
      try { first.state = 'toString'; } catch (error) { invalidRejected = error instanceof RangeError; }
      first.setAttribute('state', 'not-a-state');
      const invalidAttribute = first.getAttribute('aria-invalid');
      second.refreshTo('index'); second.remove();
      await new Promise((resolve) => setTimeout(resolve, 200));
      const cancelled = second.state === 'sleep';
      first.remove();
      const surface = createEpaperSurface({ state: 'access' });
      const sharedRenderer = surface.canvas.toDataURL() === el.canvas.toDataURL();
      let notifications = 0;
      const unsubscribe = surface.subscribe(() => { notifications += 1; });
      surface.setState('sleep'); surface.setState('sleep'); unsubscribe(); surface.setState('ready'); surface.dispose();
      return { width:canvas.width, height:canvas.height, colors:[...colors].sort(), monochromeText, independent, invalidRejected,
        invalidAttribute, cancelled, sharedRenderer, notifications, injected: first.shadowRoot.querySelectorAll('img').length };
    });
    assert.equal(component.width,296); assert.equal(component.height,160);
    assert.deepEqual(component.colors,['196,29,34,255','245,243,233,255','248,210,0,255','25,25,24,255'].sort());
    for (const key of ['monochromeText','independent','invalidRejected','cancelled','sharedRenderer']) assert.equal(component[key],true,key);
    assert.equal(component.invalidAttribute,'true'); assert.equal(component.notifications,1); assert.equal(component.injected,0);
    console.log('PASS: reusable 296x160 component, exact four colours, independent instances, safe custom text, validation, disconnect cleanup, shared canvas renderer');

    const contrasts = await page.evaluate(() => {
      const canvas=document.createElement('canvas'); canvas.width=canvas.height=1; const ctx=canvas.getContext('2d');
      function light(token) {
        ctx.fillStyle=getComputedStyle(document.documentElement).getPropertyValue(token).trim();ctx.fillRect(0,0,1,1);
        return Array.from(ctx.getImageData(0,0,1,1).data).slice(0,3).reduce((sum,v,i)=>{
          const c=v/255;return sum+(c<=0.04045?c/12.92:((c+0.055)/1.055)**2.4)*[0.2126,0.7152,0.0722][i];
        },0);
      }
      return [['--ink','--paper'],['--muted','--paper'],['--muted','--surface'],['--red','--surface'],['--on-red','--red']].map(([a,b])=>{
        const x=light(a),y=light(b);return{pair:`${a}/${b}`,ratio:(Math.max(x,y)+.05)/(Math.min(x,y)+.05)};
      });
    });
    for(const {pair,ratio}of contrasts)assert.ok(ratio>=4.5,`${pair}: ${ratio}`);
    for (const [name,width,height] of [['desktop',1440,1000],['tablet',820,1100],['mobile',390,844],['narrow',320,740]]) {
      await page.setViewportSize({width,height});
      await page.evaluate(()=>scrollTo({top:0,behavior:'instant'}));
      await page.evaluate(async()=>{await document.fonts.ready;await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));});
      await page.screenshot({path:path.join(output,`${name}.png`)});
      await scrollCollection(page,1.6/6);
      await page.waitForFunction(()=>document.querySelector('#model-host').dataset.loaded==='1');
      await page.screenshot({path:path.join(output,`${name}-orbit.png`)});
      const dimensions=await page.evaluate(()=>({content:document.documentElement.scrollWidth,viewport:innerWidth}));
      assert.ok(dimensions.content<=dimensions.viewport,`${name}: horizontal overflow`);
      const smallTargets = await page.locator('a,button,summary').evaluateAll(elements => elements.flatMap(element => {
        const rect = element.getBoundingClientRect();
        if (!rect.width || !rect.height || element.closest('[hidden]')) return [];
        return rect.width < 44 || rect.height < 44 ? [{ label: element.textContent.trim(), width:rect.width, height:rect.height }] : [];
      }));
      assert.deepEqual(smallTargets, [], `${name}: controls smaller than 44px`);
      for (const source of detailCases) await checkDetailsModal(page, name, source);
      await checkCatalogue(page, name);
      for (const section of ['file-formats', 'sources', 'transmissions']) {
        await page.locator(`#${section}`).evaluate(element => scrollTo({top:scrollY+element.getBoundingClientRect().top-24,behavior:'instant'}));
        await page.screenshot({path:path.join(output, `${name}-${section}.png`)});
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),true);
      }
      const readerLink = page.getByRole('link', {name:'Kindle and reader compatibility'});
      const originalUrl = page.url();
      await readerLink.click();
      await page.getByRole('dialog', {name:'What about Kindle?', exact:true}).waitFor();
      assert.equal(page.url(), originalUrl);
      await page.keyboard.press('Escape');
      assert.equal(await readerLink.evaluate(element => element === document.activeElement), true);
      const ids = await page.locator('[id]').evaluateAll(elements => elements.map(element => element.id));
      assert.equal(new Set(ids).size, ids.length);
      console.log(`${name}: ${await page.evaluate(() => document.documentElement.scrollHeight)}px page height`);
    }
    await page.setViewportSize({width:1440,height:1000});
    await page.locator('.field-scenes').scrollIntoViewIfNeeded();
    await page.waitForFunction(() => [...document.querySelectorAll('.field-scenes img')].every(img => img.complete && img.naturalWidth > 0));
    await page.screenshot({path:path.join(output,'access.png')});
    await page.getByRole('button', {name:'Build details', exact:true}).click();
    await page.getByRole('dialog', {name:'Build details', exact:true}).waitFor();
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('a[href="README.md"]').count(),0);
    await page.locator('#system').scrollIntoViewIfNeeded();
    await page.screenshot({path:path.join(output,'system.png')});
    assert.deepEqual(errors,[]);assert.deepEqual(failed,[]);assert.deepEqual(external,[]);
    console.log('PASS: four viewports, accessible library modal, focus trap/restore, Escape/close/backdrop, inert background, scroll lock, contrast, no page errors/failed or external requests');

    const reduced=await browser.newContext({reducedMotion:'reduce',viewport:{width:390,height:844}});
    const reducedPage=await reduced.newPage(),modelRequests=[];
    reducedPage.on('request',r=>{if(r.url().endsWith('.glb'))modelRequests.push(r.url());});
    await reducedPage.goto(base);
    await reducedPage.getByText('Static view · reduced motion',{exact:true}).waitFor();
    assert.equal(await reducedPage.locator('#collection-orbit').isVisible(),false);
    assert.equal(await reducedPage.locator('#collection-count').textContent(),'6 / 6 collections');
    assert.equal(await reducedPage.locator('.collection-step').count(),6);
    assert.match(await reducedPage.locator('#model-fallback').evaluate(e=>e.currentSrc),/model-ready.webp/);
    await reducedPage.locator('[data-screen="index"]').focus();await reducedPage.keyboard.press('Enter');
    await reducedPage.waitForFunction(()=>document.querySelector('#epaper').state==='index');
    for (const source of detailCases) await checkDetailsModal(reducedPage, 'reduced-motion', source);
    assert.equal(await reducedPage.locator('.modal-panel').evaluate(element => getComputedStyle(element).animationName), 'none');
    assert.deepEqual(modelRequests,[]);await reduced.close();
    console.log('PASS: reduced motion retains all six collections and keyboard display controls, no model download');

    const fallback=await browser.newPage({viewport:{width:1000,height:800}});
    await fallback.route('**/*.glb',route=>route.abort());await fallback.goto(base);
    await fallback.getByText('Static view · 3D unavailable',{exact:true}).waitFor();
    assert.equal(await fallback.locator('.assembly-controls').isVisible(),false);
    assert.equal(await fallback.locator('.collection-step').count(),6);await fallback.close();
    const noJS=await browser.newContext({javaScriptEnabled:false,viewport:{width:390,height:844}});
    const staticPage=await noJS.newPage();await staticPage.goto(base);
    assert.equal(await staticPage.locator('#epaper-fallback').isVisible(),true);
    assert.equal(await staticPage.locator('#model-fallback').isVisible(),true);
    assert.equal(await staticPage.locator('[data-screen="access"]').isDisabled(),true);
    assert.equal(await staticPage.locator('.collection-step').count(),6);
    assert.equal(await staticPage.locator('[data-dialog-source]').count(),0);
    for (const source of detailCases) {
      await staticPage.locator(`#${source.id} summary`).click();
      assert.equal(await staticPage.locator(`#${source.id}`).getAttribute('open'),'');
      assert.equal(await staticPage.locator(`#${source.id} p`).first().isVisible(),true);
    }
    assert.equal(await staticPage.locator('#battery-estimate .battery-table').isVisible(),true);
    assert.equal(await staticPage.locator('#format-details a[download]').getAttribute('href'),'assets/data/xberg-formats.json');
    await noJS.close();
    console.log('PASS: failed 3D and JavaScript-off preserve readable story and static assets');
    const deepLink = await browser.newPage({reducedMotion:'reduce', viewport:{width:390,height:844}});
    await deepLink.goto(new URL('#kindle-details', base).href);
    await deepLink.getByRole('dialog', {name:'What about Kindle?', exact:true}).waitFor();
    await deepLink.keyboard.press('Escape');
    assert.equal(await deepLink.getByRole('button', {name:'What about Kindle?', exact:true}).evaluate(element => element === document.activeElement),true);
    await deepLink.evaluate(() => { location.hash = 'build-details'; });
    await deepLink.getByRole('dialog', {name:'Build details', exact:true}).waitFor();
    await deepLink.keyboard.press('Escape');
    await deepLink.close();
    const unavailableCatalogue = await browser.newPage({reducedMotion:'reduce',viewport:{width:390,height:844}});
    await unavailableCatalogue.route('**/assets/data/xberg-formats.json', route => route.abort());
    await unavailableCatalogue.goto(base);
    await unavailableCatalogue.getByRole('button', {name:'Browse every file type',exact:true}).click();
    await unavailableCatalogue.getByText('The catalogue could not be loaded. Download the complete reference list below.',{exact:true}).waitFor();
    assert.equal(await unavailableCatalogue.locator('#format-catalogue fieldset').isVisible(),false);
    assert.equal(await unavailableCatalogue.locator('#format-catalogue a[download]').isVisible(),true);
    await unavailableCatalogue.keyboard.press('Escape');
    await unavailableCatalogue.close();
    console.log('PASS: seven modals, complete searchable Xberg catalogue, code grammars/MIME aliases, source and transmission sections, deep links, and no-JS/failed-data fallback');
    console.log(`Screenshots: ${output}`);
  } finally { await browser.close(); }
})().catch(error=>{console.error(error);process.exitCode=1;});
