// Screenshots of the real app for the manifest and the README, taken from
// the production build (`npm run build && npm run preview` first).
//   node scripts/screenshots.mjs
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const root = resolve(import.meta.dirname, '..');
const shots = resolve(root, 'web/public/screenshots');
const docs = resolve(root, 'docs');
mkdirSync(shots, { recursive: true });
const base = process.env.BASE_URL ?? 'http://localhost:4173';
const twins = ['scott-kelly.jpg', 'mark-kelly.jpg'].map((n) => resolve(root, 'tests/fixtures', n));

const browser = await chromium.launch({
  args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-webgpu-adapter=swiftshader', '--use-angle=swiftshader'],
});

async function shoot({ viewport, scale, path, scheme = 'light', mobile = false, scroll = 0 }) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: scale, colorScheme: scheme, isMobile: mobile, hasTouch: mobile });
  await page.goto(`${base}/#p=ig-portrait`);
  await page.waitForSelector('body[data-state=ready]');
  await page.setInputFiles('#pick-both', twins);
  await page.waitForFunction(() => document.body.dataset.photos === '2' && document.body.dataset.state === 'ready');
  await page.click('#align');
  await page.waitForFunction(() => document.querySelector('#toasts')?.textContent?.includes('Faces aligned'), null, { timeout: 120_000 });
  await page.evaluate(() => document.querySelectorAll('.toast').forEach((t) => t.remove()));
  await page.addStyleTag({ content: '.toasts { display: none !important }' });
  await page.evaluate((y) => window.scrollTo(0, y), scroll);
  await page.waitForTimeout(400);
  await page.screenshot(path.endsWith('.jpg') ? { path, type: 'jpeg', quality: 86 } : { path });
  console.log(path);
  await page.close();
}

await shoot({ viewport: { width: 1280, height: 800 }, scale: 1, path: resolve(shots, 'wide.png') });
await shoot({ viewport: { width: 414, height: 896 }, scale: 2, path: resolve(shots, 'narrow.png'), mobile: true });
await shoot({ viewport: { width: 1280, height: 800 }, scale: 2, path: resolve(docs, 'app-light.jpg') });
await shoot({ viewport: { width: 1280, height: 800 }, scale: 2, path: resolve(docs, 'app-dark.jpg'), scheme: 'dark' });
await shoot({ viewport: { width: 390, height: 844 }, scale: 3, path: resolve(docs, 'app-phone.jpg'), mobile: true, scheme: 'dark' });
await browser.close();
