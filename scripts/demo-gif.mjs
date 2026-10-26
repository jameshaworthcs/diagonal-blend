// The README's demo GIF: align the sample twins in the real app, export the
// animated reveal as WebM, and convert it with ffmpeg.
//   npm run build && npm run preview, then: node scripts/demo-gif.mjs
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';

const root = resolve(import.meta.dirname, '..');
const base = process.env.BASE_URL ?? 'http://localhost:4173';
const work = mkdtempSync(join(tmpdir(), 'blend-gif-'));
const webm = join(work, 'reveal.webm');
const gif = resolve(root, 'docs/demo.gif');

const browser = await chromium.launch({
  args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-webgpu-adapter=swiftshader', '--use-angle=swiftshader'],
});
const page = await browser.newPage({ acceptDownloads: true, viewport: { width: 1280, height: 860 } });
// Portrait preset, sRGB, a slight feather, 2.4 s sweep with 0.5 s holds, there and back.
await page.goto(`${base}/#p=ig-portrait&c=srgb&e=s,0.025&r=2.4,0.5,1`);
await page.waitForSelector('body[data-state=ready]');
await page.setInputFiles('#pick-both', ['scott-kelly.jpg', 'mark-kelly.jpg'].map((n) => resolve(root, 'tests/fixtures', n)));
await page.waitForFunction(() => document.body.dataset.photos === '2' && document.body.dataset.state === 'ready');
await page.click('#align');
await page.waitForFunction(() => document.querySelector('#toasts')?.textContent?.includes('Faces aligned'), null, { timeout: 120_000 });
await page.locator('input[name=video][value=webm]').evaluate((el) => el.click());
const [download] = await Promise.all([page.waitForEvent('download', { timeout: 300_000 }), page.click('#save-video')]);
await download.saveAs(webm);
await browser.close();

// Two passes: a palette from the whole clip, then dithered frames.
const filters = 'fps=12,scale=400:-1:flags=lanczos';
execFileSync('ffmpeg', ['-loglevel', 'error', '-y', '-i', webm, '-vf', `${filters},palettegen=max_colors=160:stats_mode=diff`, join(work, 'palette.png')]);
execFileSync('ffmpeg', [
  '-loglevel', 'error', '-y', '-i', webm, '-i', join(work, 'palette.png'),
  '-lavfi', `${filters} [x]; [x][1:v] paletteuse=dither=sierra2_4a:diff_mode=rectangle`,
  '-loop', '0', gif,
]);
console.log(gif);
