import { expect, test } from '@playwright/test';
import { canvas, fixture, load, open } from './helpers';

test('align faces lines up the twins down the midline', async ({ page }) => {
  await open(page, 'p=ig-portrait');
  await load(page, fixture('scott-kelly.jpg'), fixture('mark-kelly.jpg'));
  await page.click('#align');
  await expect(page.locator('#toasts')).toContainText('Faces aligned', { timeout: 90_000 });
  // The split is now near vertical and B has moved.
  await expect.poll(() => page.evaluate(() => location.hash)).toMatch(/b=/);
  const angle = Number(await page.locator('#angle').inputValue());
  expect(Math.abs(angle)).toBeLessThan(10);
  // Eye markers are drawn for both photos, on top of each other.
  await expect(page.locator('#eyes circle.eye-a')).toHaveCount(2);
  await expect(page.locator('#eyes circle.eye-b')).toHaveCount(2);
  const [a, b] = await Promise.all(
    ['eye-a', 'eye-b'].map((c) => page.locator(`#eyes circle.${c}`).first().evaluate((el) => [Number(el.getAttribute('cx')), Number(el.getAttribute('cy'))])),
  );
  expect(Math.hypot(a[0] - b[0], a[1] - b[1])).toBeLessThan(0.5);
  await expect(await canvas(page)).toHaveScreenshot('aligned-twins.png', { maxDiffPixelRatio: 0.01 });
});

test('no face leaves everything as it was', async ({ page }) => {
  await open(page, 's=20,0');
  await load(page, fixture('pattern-a.png'), fixture('mark-kelly.jpg'));
  const before = await page.evaluate(() => location.hash);
  await page.click('#align');
  await expect(page.locator('#toasts')).toContainText('No face found in photo A', { timeout: 90_000 });
  expect(await page.evaluate(() => location.hash)).toBe(before);
  // B's eyes were found and may be marked; A has none.
  await expect(page.locator('#eyes circle.eye-a')).toHaveCount(0);
});
