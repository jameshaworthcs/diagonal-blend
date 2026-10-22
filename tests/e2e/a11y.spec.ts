import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { fixture, load, open } from './helpers';

for (const scheme of ['light', 'dark'] as const) {
  test(`no axe violations, ${scheme} theme @mobile`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await open(page);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
}

test('the split handles are keyboard sliders', async ({ page }) => {
  await open(page, 's=10,0');
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  const rot = page.getByRole('slider', { name: 'Split angle' });
  const pos = page.getByRole('slider', { name: 'Split position' });
  await expect(rot).toHaveAttribute('aria-valuemin', '-180');
  await expect(rot).toHaveAttribute('aria-valuemax', '180');
  await rot.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  await expect(rot).toHaveAttribute('aria-valuenow', '12.0');
  await page.keyboard.press('Shift+ArrowLeft');
  await expect(rot).toHaveAttribute('aria-valuenow', '2.0');
  await expect(rot).toHaveAttribute('aria-valuetext', '2 degrees');
  await pos.focus();
  await page.keyboard.press('End');
  await expect(pos).toHaveAttribute('aria-valuenow', '100.0');
  await page.keyboard.press('Shift+ArrowLeft');
  await expect(pos).toHaveAttribute('aria-valuenow', '90.0');
  await expect(pos).toHaveAttribute('aria-valuetext', '90% towards B');
  await expect.poll(() => page.evaluate(() => location.hash)).toMatch(/^#s=2,/);
});

test('tab reaches every control with a visible focus ring', async ({ page }) => {
  await open(page);
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  // Start from the top of the document, as a fresh keyboard user would.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  const seen = new Set<string>();
  for (let i = 0; i < 80; i++) {
    await page.keyboard.press('Tab');
    const info = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      // Radios draw their ring on the label's span, hidden file inputs on
      // their label.
      const ring = el.matches('.seg input') ? el.nextElementSibling! : el.matches('label > .visually-hidden') ? el.parentElement! : el;
      const style = getComputedStyle(ring);
      const input = el as HTMLInputElement;
      if (el === document.body) return { id: 'BODY', outline: '' };
      const id = el.id || (input.name ? `${input.name}=${input.value}` : '') || el.getAttribute('aria-label') || el.textContent?.trim() || el.tagName;
      return { id, outline: style.outlineStyle };
    });
    // Stop once focus has gone round the whole page.
    if (info.id === 'BODY' || seen.has(info.id)) break;
    seen.add(info.id);
    expect(info.outline, `focus ring on ${info.id}`).not.toBe('none');
  }
  for (const id of ['handle-pos', 'handle-rot', 'align', 'angle', 'zoom', 'preset', 'save-still']) {
    expect([...seen]).toContain(id);
  }
  // Nothing hidden is reachable.
  expect([...seen]).not.toContain('custom-w');
  expect([...seen]).not.toContain('install');
});

test('reduced motion turns off the reveal preview', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await open(page);
  await load(page, fixture('pattern-a.png'), fixture('pattern-b.png'));
  await expect(page.locator('#preview-reveal')).toBeDisabled();
  await expect(page.locator('#save-video')).toBeEnabled();
});
