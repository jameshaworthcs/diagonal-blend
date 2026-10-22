import { readFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { deflateSync } from 'node:zlib';
import { expect, type Download, type Page } from '@playwright/test';

export const fixture = (name: string) => resolve(import.meta.dirname, '../fixtures', name);

/** Open the app with a settings hash and wait for the renderer. */
export async function open(page: Page, hash = '') {
  await page.goto(`/${hash ? `#${hash}` : ''}`);
  await expect(page.locator('body')).toHaveAttribute('data-state', 'ready', { timeout: 60_000 });
}

export async function frames(page: Page, n = 3) {
  await page.evaluate(
    (n) =>
      new Promise<void>((done) => {
        const step = (k: number) => (k === 0 ? done() : requestAnimationFrame(() => step(k - 1)));
        step(n);
      }),
    n,
  );
}

type Input = string | { name: string; mimeType: string; buffer: Buffer };

const mime = (f: string) => (f.endsWith('.png') ? 'image/png' : f.endsWith('.webp') ? 'image/webp' : 'image/jpeg');

/** Choose a radio button or checkbox through the DOM, as a click on its
 * label would, even where the sticky preview covers it on a phone. */
export async function pick(page: Page, selector: string) {
  await page.locator(selector).evaluate((el) => (el as HTMLInputElement).click());
}

/** Load photos A and B through the file picker. */
export async function load(page: Page, a: Input, b: Input) {
  const file = (f: Input) => (typeof f === 'string' ? { name: basename(f), mimeType: mime(f), buffer: readFileSync(f) } : f);
  await page.setInputFiles('#pick-both', [file(a), file(b)]);
  await expect(page.locator('body')).toHaveAttribute('data-photos', '2');
  await expect(page.locator('body')).toHaveAttribute('data-state', 'ready');
  await frames(page);
}

/** The bare canvas, with the handles and overlays hidden. */
export async function canvas(page: Page) {
  await page.addStyleTag({ content: '.handle, .overlay, .toasts { visibility: hidden !important; }' });
  await frames(page);
  return page.locator('#canvas');
}

export async function download(page: Page, click: () => Promise<void>): Promise<{ file: Download; bytes: Buffer }> {
  const [file] = await Promise.all([page.waitForEvent('download', { timeout: 300_000 }), click()]);
  const path = await file.path();
  return { file, bytes: readFileSync(path) };
}

export async function exportStill(page: Page, format: 'png' | 'webp' | 'avif') {
  await pick(page, `input[name=still][value=${format}]`);
  return download(page, () => page.click('#save-still'));
}

/** Decode an image with the browser and return its RGBA pixels. */
export async function decodePixels(page: Page, bytes: Buffer, type: string) {
  const out = await page.evaluate(
    async ({ b64, type }) => {
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bin], { type }), { colorSpaceConversion: 'none' });
      const ctx = new OffscreenCanvas(bmp.width, bmp.height).getContext('2d')!;
      ctx.drawImage(bmp, 0, 0);
      const data = ctx.getImageData(0, 0, bmp.width, bmp.height).data;
      let s = '';
      for (let i = 0; i < data.length; i += 0x8000) s += String.fromCharCode(...data.subarray(i, i + 0x8000));
      return { width: bmp.width, height: bmp.height, data: btoa(s) };
    },
    { b64: bytes.toString('base64'), type },
  );
  return { width: out.width, height: out.height, data: Buffer.from(out.data, 'base64') };
}

/** A flat-colour PNG, built here so the edge tests have clean inputs. */
export function flatPng(name: string, w: number, h: number, rgb: [number, number, number]) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (b: Buffer) => {
    let c = 0xffffffff;
    for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type: string, data: Buffer) => {
    const t = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(t));
    return Buffer.concat([len, t, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => rgb).flat())]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  const png = Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
  return { name, mimeType: 'image/png', buffer: png };
}
