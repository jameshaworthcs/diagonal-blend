// Visual regression of the preview and the full-resolution export, plus an
// analytic check that the hard edge is one pixel wide at any angle.

import { expect, test } from '@playwright/test';
import { canvas, decodePixels, exportStill, fixture, flatPng, load, open } from './helpers';

test.describe('preview', () => {
  test('hard diagonal split', async ({ page }) => {
    await open(page, 's=28,0&p=ig-square');
    await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
    await expect(await canvas(page)).toHaveScreenshot('hard-28.png');
  });

  test('feathered blend', async ({ page }) => {
    await open(page, 's=-40,0.08&e=s,0.2&p=x');
    await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
    await expect(await canvas(page)).toHaveScreenshot('soft-40.png');
  });

  test('panned, zoomed and rotated photos', async ({ page }) => {
    await open(page, 's=12,-0.05&a=0.12,-0.04,1.6,18&b=-0.2,0.1,0.7,-30&p=story');
    await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
    await expect(await canvas(page)).toHaveScreenshot('placed.png');
  });

  test('EXIF orientation is honoured', async ({ page }) => {
    await open(page, 's=90,0.3&p=ig-portrait');
    await load(page, fixture('rotated-6.jpg'), fixture('pattern-b.png'));
    await expect(page.locator('#info-0')).toContainText('400×600');
    await expect(page.locator('#info-0')).toContainText('EXIF 6');
    await expect(await canvas(page)).toHaveScreenshot('exif-6.png');
  });
});

test.describe('export', () => {
  test('png at full resolution matches the reference', async ({ page }) => {
    await open(page, 's=33,0.02&e=s,0.05&c=srgb&p=ig-portrait');
    await load(page, fixture('scott-kelly.jpg'), fixture('mark-kelly.jpg'));
    const { file, bytes } = await exportStill(page, 'png');
    expect(file.suggestedFilename()).toBe('diagonal-blend-1080x1350.png');
    expect(bytes.readUInt32BE(16)).toBe(1080);
    expect(bytes.readUInt32BE(20)).toBe(1350);
    expect(bytes.subarray(0, 120).includes('iCCP')).toBe(true);
    expect(bytes).toMatchSnapshot('export-1080x1350.png');
  });

  for (const angle of [0, 17, 45, 71]) {
    test(`hard edge is one pixel wide at ${angle}°`, async ({ page }) => {
      await open(page, `s=${angle},0.013&c=srgb&p=400x400`);
      await load(page, flatPng('red.png', 64, 64, [255, 0, 0]), flatPng('blue.png', 64, 64, [0, 0, 255]));
      const { bytes } = await exportStill(page, 'png');
      const { width, height, data } = await decodePixels(page, bytes, 'image/png');
      expect([width, height]).toEqual([400, 400]);
      // Work along rows for steep lines and columns for shallow ones.
      const steep = Math.abs(Math.cos((angle * Math.PI) / 180)) >= Math.SQRT1_2;
      const slope = steep ? 1 / Math.cos((angle * Math.PI) / 180) : 1 / Math.sin((angle * Math.PI) / 180);
      const edges: number[] = [];
      for (let line = 20; line < 380; line++) {
        let partial = 0;
        let coverage = 0;
        for (let k = 0; k < 400; k++) {
          const i = (steep ? line * 400 + k : k * 400 + line) * 4;
          // Blue is 0 in A and 1 in B; the blend is in linear light, so
          // undo the sRGB encoding to read the coverage.
          const e = data[i + 2] / 255;
          const b = e <= 0.04045 ? e / 12.92 : ((e + 0.055) / 1.055) ** 2.4;
          coverage += b;
          if (b > 0.02 && b < 0.98) partial++;
        }
        // Only as many in-between pixels as one pixel of width crosses.
        expect(partial).toBeLessThanOrEqual(Math.ceil(Math.abs(slope)) + 1);
        edges.push(400 - coverage);
      }
      // The sub-pixel edge position moves in a straight line: no stairs.
      const n = edges.length;
      const xs = edges.map((_, i) => i);
      const mx = xs.reduce((a, b) => a + b) / n;
      const my = edges.reduce((a, b) => a + b) / n;
      const m = xs.reduce((s, x, i) => s + (x - mx) * (edges[i] - my), 0) / xs.reduce((s, x) => s + (x - mx) ** 2, 0);
      const worst = Math.max(...edges.map((y, i) => Math.abs(y - (my + m * (xs[i] - mx)))));
      expect(worst).toBeLessThan(0.15);
    });
  }
});
