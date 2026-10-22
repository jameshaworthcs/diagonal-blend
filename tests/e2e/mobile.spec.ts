import { expect, test } from '@playwright/test';
import { fixture, frames, load, open, pick } from './helpers';

test('phone layout puts the preview above the controls @mobile-only', async ({ page }) => {
  await open(page, 'p=ig-portrait');
  await load(page, fixture('scott-kelly.jpg'), fixture('mark-kelly.jpg'));
  const stage = await page.locator('#stage').boundingBox();
  const panel = await page.locator('#controls').boundingBox();
  expect(panel!.y).toBeGreaterThanOrEqual(stage!.y + stage!.height - 1);
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width).toBeLessThanOrEqual(page.viewportSize()!.width);
  for (const id of ['#handle-pos', '#handle-rot']) {
    const b = await page.locator(id).boundingBox();
    expect(Math.min(b!.width, b!.height)).toBeGreaterThanOrEqual(36);
  }
});

test('two-finger pinch zooms and twists the photo under the fingers @mobile-only', async ({ page }) => {
  await open(page, 's=90,0.45');
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  const box = (await page.locator('#canvas').boundingBox())!;
  const cx = box.width / 2;
  const cy = box.height * 0.3;
  // Synthetic touch pointers: spread from 60 px to 120 px apart and turn 30°.
  await page.evaluate(
    async ({ cx, cy }) => {
      const c = document.getElementById('canvas')!;
      const r = c.getBoundingClientRect();
      const fire = (type: string, id: number, x: number, y: number) =>
        c.dispatchEvent(new PointerEvent(type, { pointerId: id, pointerType: 'touch', isPrimary: id === 1, clientX: r.left + x, clientY: r.top + y, bubbles: true }));
      fire('pointerdown', 1, cx - 30, cy);
      fire('pointerdown', 2, cx + 30, cy);
      const steps = 12;
      for (let k = 1; k <= steps; k++) {
        const t = k / steps;
        const d = 30 + 30 * t;
        const a = (30 * t * Math.PI) / 180;
        fire('pointermove', 2, cx + d * Math.cos(a), cy + d * Math.sin(a));
        await new Promise((r) => requestAnimationFrame(r));
      }
      fire('pointerup', 2, cx + 60 * Math.cos(Math.PI / 6), cy + 60 * Math.sin(Math.PI / 6));
      fire('pointerup', 1, cx - 30, cy);
    },
    { cx, cy },
  );
  await frames(page);
  // With the split near the bottom, the gesture lands on A.
  await pick(page, 'input[name=target][value="0"]');
  const zoom = parseFloat((await page.locator('#zoom-out').textContent())!);
  const rot = parseFloat((await page.locator('#rotate-out').textContent())!);
  expect(zoom).toBeGreaterThan(1.3);
  expect(zoom).toBeLessThan(1.7);
  expect(rot).toBeGreaterThan(10);
  expect(rot).toBeLessThan(35);
});
