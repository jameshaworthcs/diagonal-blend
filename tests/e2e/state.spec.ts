import { expect, test } from '@playwright/test';
import { fixture, load, open, pick } from './helpers';

test('settings come back from the url', async ({ page }) => {
  await open(page, 's=-33.5,0.1&e=s,0.125&p=story&c=srgb&r=4.5,1.2,0');
  await expect(page.locator('#angle')).toHaveValue('-33.5');
  await expect(page.locator('input[name=edge][value=soft]')).toBeChecked();
  await expect(page.locator('#feather')).toHaveValue('12.5');
  await expect(page.locator('#preset')).toHaveValue('story');
  await expect(page.locator('#color')).toHaveValue('srgb');
  await expect(page.locator('#sweep')).toHaveValue('4.5');
  await expect(page.locator('#round-trip')).not.toBeChecked();
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  const box = await page.locator('#canvas').boundingBox();
  expect(box!.width / box!.height).toBeCloseTo(1080 / 1920, 2);
});

test('changes are written to the url, and nothing else is', async ({ page }) => {
  await open(page);
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  await page.locator('#angle').fill('45');
  await pick(page, 'input[name=edge][value=soft]');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#s=45,0&e=s,0.08');
  // Reloading with that hash keeps the settings.
  await page.reload();
  await expect(page.locator('body')).toHaveAttribute('data-state', 'ready');
  await expect(page.locator('#angle')).toHaveValue('45');
});

test('a placement from the link applies to the first photo loaded', async ({ page }) => {
  await open(page, 'a=0.1,-0.05,1.5,10');
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  await expect.poll(() => page.evaluate(() => location.hash)).toContain('a=0.1,-0.05,1.5,10');
  await pick(page, 'input[name=target][value="0"]');
  await expect(page.locator('#zoom-out')).toHaveText('1.50×');
});
