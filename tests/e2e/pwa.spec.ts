import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import { fixture, open } from './helpers';

test.describe('installable and offline', () => {
  test.skip(({ browserName }) => browserName !== 'chromium', 'service worker checks run in Chromium');

  test('manifest declares icons, screenshots and the share target', async ({ request }) => {
    const m = await (await request.get('/manifest.webmanifest')).json();
    expect(m.display).toBe('standalone');
    expect(m.icons.some((i: { purpose: string }) => i.purpose === 'maskable')).toBe(true);
    expect(m.screenshots.map((s: { form_factor: string }) => s.form_factor).sort()).toEqual(['narrow', 'wide']);
    expect(m.share_target).toMatchObject({ action: '/share-target', method: 'POST', enctype: 'multipart/form-data' });
    for (const icon of m.icons) expect((await request.get(icon.src)).ok()).toBe(true);
    for (const shot of m.screenshots) expect((await request.get(shot.src)).ok()).toBe(true);
  });

  test('works offline after the first visit', async ({ page, context }) => {
    await open(page);
    await page.evaluate(() => navigator.serviceWorker.ready.then(() => true));
    await expect(page.locator('#toasts')).toContainText('Ready to work offline', { timeout: 60_000 });
    await context.setOffline(true);
    await page.reload();
    await expect(page.locator('body')).toHaveAttribute('data-state', 'ready', { timeout: 60_000 });
    await page.setInputFiles('#pick-both', [fixture('scott-kelly.jpg'), fixture('mark-kelly.jpg')]);
    await expect(page.locator('body')).toHaveAttribute('data-photos', '2');
    // The face detector loads from the precache too.
    await page.click('#align');
    await expect(page.locator('#toasts')).toContainText('Faces aligned', { timeout: 90_000 });
  });

  test('two photos shared to the app arrive in A and B', async ({ page }) => {
    await open(page);
    await page.evaluate(() => navigator.serviceWorker.ready.then(() => true));
    await page.reload(); // now controlled by the worker
    await expect(page.locator('body')).toHaveAttribute('data-state', 'ready');
    const files = ['scott-kelly.jpg', 'mark-kelly.jpg'].map((n) => ({ n, b64: readFileSync(fixture(n)).toString('base64') }));
    // What the OS share sheet does: a multipart POST to the share target.
    const landed = await page.evaluate(async (files) => {
      const form = new FormData();
      form.append('title', 'shared');
      for (const { n, b64 } of files) {
        const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
        form.append('photos', new File([bin], n, { type: 'image/jpeg' }));
      }
      const res = await fetch('/share-target', { method: 'POST', body: form });
      return res.url;
    }, files);
    expect(new URL(landed).search).toBe('?shared=2');
    await page.goto(landed);
    await expect(page.locator('body')).toHaveAttribute('data-photos', '2', { timeout: 60_000 });
    await expect(page.locator('#name-0')).toHaveText('scott-kelly.jpg');
    await expect(page.locator('#name-1')).toHaveText('mark-kelly.jpg');
    expect(new URL(page.url()).search).toBe('');
  });
});
