// On a Display P3 screen the preview swapchain is P3 too, so values reach
// the display unclipped. These run in the projects that force a P3 profile.

import { expect, test } from '@playwright/test';
import { canvas, fixture, flatPng, load, open } from './helpers';

test('the preview presents display p3 values @p3-only', async ({ page }) => {
  await open(page, 's=0,0&p=ig-square');
  await load(page, fixture('p3-red.png'), flatPng('red.png', 8, 8, [255, 0, 0]));
  await expect(page.locator('body')).toHaveAttribute('data-color-space', 'display-p3');
  const shot = await (await canvas(page)).screenshot();
  const box = (await page.locator('#canvas').boundingBox())!;
  const px = await page.evaluate(
    async ({ b64, xs, y }) => {
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bin], { type: 'image/png' }), { colorSpaceConversion: 'none' });
      const ctx = new OffscreenCanvas(bmp.width, bmp.height).getContext('2d')!;
      ctx.drawImage(bmp, 0, 0);
      return xs.map((x) => [...ctx.getImageData(x, y, 1, 1).data.slice(0, 3)]);
    },
    { b64: shot.toString('base64'), xs: [Math.round(box.width * 0.25), Math.round(box.width * 0.75)], y: Math.round(box.height / 2) },
  );
  // P3 red is the P3 primary itself; sRGB red sits inside the P3 gamut.
  expect(px[0]).toEqual([255, 0, 0]);
  px[1].forEach((v, k) => expect(Math.abs(v - [234, 51, 35][k])).toBeLessThanOrEqual(1));
});
