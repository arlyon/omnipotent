/* Uses the existing Playwright/browser installation with Chromium sandboxing. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { chromium } = require(process.env.PLAYWRIGHT_PATH || 'playwright');
const base = process.env.SITE_URL || 'http://127.0.0.1:8000';
const output = process.env.SCREENSHOT_DIR || '/tmp/omnipotent-gallery-checks';
fs.mkdirSync(output, { recursive: true });

(async () => {
  const browser = await chromium.launch({ headless:true, chromiumSandbox:true,
    executablePath:process.env.CHROMIUM_PATH || undefined });
  try {
    for (const width of [320, 768, 1440]) {
      const page = await browser.newPage({ viewport:{ width, height:900 }, reducedMotion:'reduce' });
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('response', response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
      await page.goto(`${base}/assets.html`);
      assert.equal(await page.getByRole('heading', { level:1 }).textContent(), 'Assets.');
      assert.equal(await page.locator('.asset-preview').count(), 31);
      for (const label of ['Cobalt studio', 'Compact red and porcelain', 'On air', 'System architecture']) {
        const opener = page.getByRole('link', { name:`Preview ${label}`, exact:true });
        await opener.click();
        const dialog = page.getByRole('dialog');
        await dialog.waitFor();
        await page.waitForFunction(() => document.querySelector('#asset-dialog-image').naturalWidth > 0);
        assert.equal(await page.locator('main').evaluate(node => node.inert), true);
        assert.equal(await page.getByRole('button', { name:'Close preview' }).evaluate(node => node === document.activeElement), true);
        assert.equal(await dialog.evaluate(node => {
          const rect = node.querySelector('.modal-panel').getBoundingClientRect();
          return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight;
        }), true);
        for (let index = 0; index < 6; index++) {
          await page.keyboard.press(index % 2 ? 'Shift+Tab' : 'Tab');
          assert.equal(await dialog.evaluate(node => node.contains(document.activeElement)), true);
        }
        if (label === 'Cobalt studio') await page.screenshot({ path:`${output}/${width}-preview.png` });
        await page.keyboard.press('Escape');
        assert.equal(await dialog.isVisible(), false);
        assert.equal(await page.locator('main').evaluate(node => node.inert), false);
        assert.equal(await opener.evaluate(node => node === document.activeElement), true);
        assert.equal(await page.evaluate(() => document.documentElement.classList.contains('modal-open')), false);
      }
      const opener = page.getByRole('link', { name:'Preview Cobalt studio', exact:true });
      await opener.focus();
      await page.keyboard.press('Enter');
      await page.getByRole('button', { name:'Close preview' }).click();
      await opener.click();
      await page.locator('.modal-backdrop').click({ position:{ x:4, y:4 } });
      assert.equal(await page.getByRole('dialog').count(), 0);
      const file = page.waitForEvent('download');
      await page.getByRole('link', { name:'Download Cobalt studio PNG', exact:true }).click();
      const download = await file;
      assert.equal(download.suggestedFilename(), '01-electric-blue.png');
      assert.equal(await download.failure(), null);
      await page.getByRole('navigation', { name:'Asset collections' }).getByRole('link', { name:'Screens 6' }).click();
      assert.equal(new URL(page.url()).hash, '#screens');
      for (const section of await page.locator('.archive-section').all()) {
        await section.scrollIntoViewIfNeeded();
        await section.locator('img').evaluateAll(nodes => Promise.all(nodes.map(node => {
          node.loading = 'eager';
          return node.decode();
        })));
      }
      await page.evaluate(() => scrollTo({ top:0, behavior:'instant' }));
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}: no horizontal page overflow`);
      await page.screenshot({ path:`${output}/${width}-archive.png`, fullPage:true });
      assert.deepEqual(errors, []);
      await page.close();
      console.log(`${width}px: layout, images, preview focus trap/restoration, keyboard, backdrop, downloads and section links passed`);
    }
    const fallback = await browser.newPage({ javaScriptEnabled:false });
    await fallback.goto(`${base}/assets.html`);
    await fallback.getByRole('link', { name:'Preview Cobalt studio', exact:true }).click();
    assert.ok(fallback.url().endsWith('/docs/assets/field-archive/stock/photos/01-electric-blue.png'));
    await fallback.close();
    const blocked = await browser.newPage();
    await blocked.route('**/a11y-dialog.esm.js', route => route.abort());
    await blocked.goto(`${base}/assets.html`);
    await blocked.getByRole('link', { name:'Preview Cobalt studio', exact:true }).click();
    assert.ok(blocked.url().endsWith('/docs/assets/field-archive/stock/photos/01-electric-blue.png'));
    await blocked.close();
    const failure = await browser.newPage();
    await failure.route('**/stock/photos/01-electric-blue.png', route => route.abort());
    await failure.goto(`${base}/assets.html`);
    await failure.getByRole('link', { name:'Preview Cobalt studio', exact:true }).click();
    await failure.locator('#asset-dialog-error').waitFor();
    assert.equal(await failure.locator('#asset-dialog-download').getAttribute('href'), `${base}/docs/assets/field-archive/stock/photos/01-electric-blue.png`);
    await failure.getByRole('button', { name:'Close preview' }).click();
    await failure.getByRole('link', { name:'Preview On air', exact:true }).click();
    await failure.waitForFunction(() => document.querySelector('#asset-dialog-image').naturalWidth > 0);
    assert.equal(await failure.locator('#asset-dialog-error').isVisible(), false);
    await failure.close();
    const home = await browser.newPage({ reducedMotion:'reduce' });
    await home.goto(base);
    await home.locator('.site-footer').getByRole('link', { name:'Assets' }).click();
    assert.ok(home.url().endsWith('/assets.html'));
    await home.getByRole('navigation', { name:'Main navigation' }).getByRole('link', { name:'Overview' }).click();
    assert.ok(home.url().endsWith('/index.html'));
    await home.close();
    console.log('No-JS, blocked-library, failed-original recovery and landing-page navigation passed');
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
