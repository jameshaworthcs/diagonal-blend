import { expect, test } from '@playwright/test';
import { decodePixels, download, exportStill, fixture, flatPng, load, open, pick } from './helpers';

test('webp carries the display p3 profile', async ({ page }) => {
  await open(page, 'p=ig-square&c=p3');
  await load(page, fixture('pattern-a.png'), fixture('p3-red.png'));
  const { file, bytes } = await exportStill(page, 'webp');
  expect(file.suggestedFilename()).toBe('diagonal-blend-1080x1080.webp');
  expect(bytes.subarray(12, 16).toString()).toBe('VP8X');
  expect(bytes[20] & 0x20).toBe(0x20);
  const icc = bytes.indexOf('ICCP');
  expect(icc).toBeGreaterThan(0);
  // The profile's description, as UTF-16BE in its mluc tag.
  const name = Buffer.from([...'Display P3'].flatMap((c) => [0, c.charCodeAt(0)]));
  expect(bytes.indexOf(name, icc)).toBeGreaterThan(icc);
});

for (const space of ['p3', 'srgb'] as const) {
  test(`colours are converted exactly for a ${space} export`, async ({ page }) => {
    // A is Display P3 red (outside sRGB; its red half fills the left of
    // the frame), B is plain sRGB red. In a P3 export, P3 red is
    // (255, 0, 0) and sRGB red has smaller coordinates; in an sRGB export,
    // sRGB red is (255, 0, 0) and P3 red clips to it.
    await open(page, `s=0,0&p=ig-square&c=${space}`);
    await load(page, fixture('p3-red.png'), flatPng('red.png', 64, 64, [255, 0, 0]));
    const { bytes } = await exportStill(page, 'png');
    const { data, width } = await decodePixels(page, bytes, 'image/png');
    const at = (x: number, y: number) => [...data.subarray((y * width + x) * 4, (y * width + x) * 4 + 3)];
    const a = at(200, 540);
    const b = at(800, 540);
    expect(a).toEqual([255, 0, 0]);
    if (space === 'srgb') expect(b).toEqual([255, 0, 0]);
    else b.forEach((v, k) => expect(Math.abs(v - [234, 51, 35][k])).toBeLessThanOrEqual(1));
  });
}

test('animated reveal encodes and can be cancelled', async ({ page, browserName }) => {
  await open(page, 'p=ig-square&r=1,0,0');
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  const webm = page.locator('[data-format=webm]');
  const mp4 = page.locator('[data-format=mp4]');
  test.skip((await webm.isHidden()) && (await mp4.isHidden()), `no WebCodecs encoder in ${browserName}`);
  const format = (await webm.isVisible()) ? 'webm' : 'mp4';
  await pick(page, `input[name=video][value=${format}]`);

  const { file, bytes } = await download(page, () => page.click('#save-video'));
  expect(file.suggestedFilename()).toBe(`diagonal-blend-reveal-1080x1080.${format}`);
  if (format === 'webm') expect([...bytes.subarray(0, 4)]).toEqual([0x1a, 0x45, 0xdf, 0xa3]);
  else expect(bytes.subarray(4, 8).toString()).toBe('ftyp');
  await expect(page.locator('#toasts')).toContainText('Saved a 1.0 s');

  // A longer one, cancelled part way.
  await page.locator('#sweep').fill('6');
  await page.click('#save-video');
  await expect(page.locator('#progress-text')).toContainText('Frame');
  await page.click('#cancel');
  await expect(page.locator('#toasts')).toContainText('cancelled');
  await expect(page.locator('#progress')).toBeHidden();
});

test('avif decodes and matches the png', async ({ page, browserName }) => {
  await open(page, 's=33,0.02&p=ig-portrait&c=p3');
  await load(page, fixture('scott-kelly.jpg'), fixture('mark-kelly.jpg'));
  test.skip(await page.locator('[data-format=avif]').isHidden(), `no AVIF encoder in ${browserName}`);
  const png = await exportStill(page, 'png');
  const avif = await exportStill(page, 'avif');
  expect(avif.file.suggestedFilename()).toBe('diagonal-blend-1080x1350.avif');
  expect(avif.bytes.subarray(4, 12).toString()).toBe('ftypavif');
  // Display P3 both as CICP (primaries 12) and as an ICC profile.
  expect(avif.bytes.indexOf(Buffer.from('nclx\0\x0c\0\x0d', 'latin1'))).toBeGreaterThan(0);
  expect(avif.bytes.indexOf('prof')).toBeGreaterThan(0);
  const a = await decodePixels(page, png.bytes, 'image/png');
  const b = await decodePixels(page, avif.bytes, 'image/avif');
  expect([b.width, b.height]).toEqual([1080, 1350]);
  let sum = 0;
  for (let i = 0; i < a.data.length; i += 4) for (let c = 0; c < 3; c++) sum += Math.abs(a.data[i + c] - b.data[i + c]);
  // Lossy and 4:2:0, but close: under 4 levels out of 255 on average.
  // (Firefox's encoder uses limited range, which costs a little more.)
  expect(sum / ((a.data.length / 4) * 3)).toBeLessThan(4);
});
