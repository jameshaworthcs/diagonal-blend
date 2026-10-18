// Turning a photo file into texture pixels.
//
// The core reads the EXIF orientation and the ICC profile straight from the
// bytes. For sRGB and Display P3 photos the browser is told not to convert
// colours (colorSpaceConversion: "none"), so the texture holds the file's own
// values and the shader converts them to the output space exactly. Anything
// else (Adobe RGB, ProPhoto...) is converted to sRGB by the browser.

import type { Core } from './core';

export interface Photo {
  name: string;
  /** Pixels as decoded, before any orientation the decoder did not apply. */
  rgba: Uint8ClampedArray;
  width: number;
  height: number;
  /** Size as displayed, after orientation. */
  displayWidth: number;
  displayHeight: number;
  /** Full displayed size of the original file, before any downscaling. */
  sourceWidth: number;
  sourceHeight: number;
  /** EXIF orientation from the file (1 if none) and what is still owed. */
  orientation: number;
  residual: number;
  /** 0 sRGB, 1 Display P3: the primaries of `rgba`. */
  space: 0 | 1;
  /** The profile we found: 'sRGB', 'Display P3', 'untagged' or 'other'. */
  profile: string;
  /** A smaller copy for the face detector, as stored in `rgba`. */
  small: ImageBitmap;
  url: string;
}

// A 2x1 JPEG tagged with orientation 6. A browser that honours the tag
// decodes it as 1x2.
const PROBE =
  '/9j/4QAiRXhpZgAATU0AKgAAAAgAAQESAAMAAAABAAYAAAAAAAD/2wBDAAg+Pkk+SVVVVVVVVWRdZGhoaGRkZGRoaGhwcHCDg4NwcHBoaHBwfHyDg4+Tj4eHg4eTk5ubm7q6srLZ2eD/////xABMAAEBAAAAAAAAAAAAAAAAAAAABgEBAQAAAAAAAAAAAAAAAAAABgcQAQAAAAAAAAAAAAAAAAAAAAARAQAAAAAAAAAAAAAAAAAAAAD/wAARCAABAAIDASIAAhEAAxEA/9oADAMBAAIRAxEAPwCLAE1/f//Z';

let autoOrients: Promise<boolean> | null = null;

/** Does createImageBitmap apply EXIF orientation by itself? Asked once. */
export function browserAutoOrients(): Promise<boolean> {
  autoOrients ??= (async () => {
    try {
      const bytes = Uint8Array.from(atob(PROBE), (c) => c.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/jpeg' }), { imageOrientation: 'from-image' });
      const rotated = bmp.width === 1 && bmp.height === 2;
      bmp.close();
      return rotated;
    } catch {
      return true;
    }
  })();
  return autoOrients;
}

const DETECT_SIZE = 1024;
/** Cap the texture so two photos stay comfortably inside a phone's memory. */
const MAX_PIXELS = 36_000_000;

function context(w: number, h: number): OffscreenCanvasRenderingContext2D {
  const ctx = new OffscreenCanvas(w, h).getContext('2d', { colorSpace: 'srgb', willReadFrequently: true });
  if (!ctx) throw new Error('2D canvas unavailable');
  return ctx;
}

export async function decodePhoto(core: Core, file: Blob, name: string, maxTexture: number): Promise<Photo> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const [tag, storedW, storedH] = core.read_image_header(bytes);
  const detected = core.detect_color_space(bytes);
  const native = detected >= 0;

  let bmp: ImageBitmap;
  try {
    bmp = await createImageBitmap(file, {
      colorSpaceConversion: native ? 'none' : 'default',
      imageOrientation: 'from-image',
      premultiplyAlpha: 'none',
    });
  } catch {
    throw new Error(`${name} is not an image this browser can open`);
  }

  const orientation = tag >= 1 && tag <= 8 ? tag : 1;
  const residual =
    orientation === 1 || !storedW
      ? 1
      : core.residual_orientation(orientation, storedW, storedH, bmp.width, bmp.height, await browserAutoOrients());

  // Fit inside the GPU's texture limit and the memory budget.
  const scale = Math.min(1, maxTexture / Math.max(bmp.width, bmp.height), Math.sqrt(MAX_PIXELS / (bmp.width * bmp.height)));
  const width = Math.max(1, Math.round(bmp.width * scale));
  const height = Math.max(1, Math.round(bmp.height * scale));
  const ctx = context(width, height);
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(bmp, 0, 0, width, height);
  const rgba = ctx.getImageData(0, 0, width, height, { colorSpace: 'srgb' }).data;

  const k = Math.min(1, DETECT_SIZE / Math.max(width, height));
  const small = await createImageBitmap(bmp, {
    resizeWidth: Math.max(1, Math.round(width * k)),
    resizeHeight: Math.max(1, Math.round(height * k)),
    resizeQuality: 'medium',
  });
  bmp.close();

  const swapped = residual >= 5;
  const [decodedW, decodedH] = [Math.round(width / scale), Math.round(height / scale)];
  return {
    name,
    rgba,
    width,
    height,
    displayWidth: swapped ? height : width,
    displayHeight: swapped ? width : height,
    sourceWidth: swapped ? decodedH : decodedW,
    sourceHeight: swapped ? decodedW : decodedH,
    orientation,
    residual,
    space: detected === 1 ? 1 : 0,
    profile: core.describe_color(bytes),
    small,
    url: URL.createObjectURL(file),
  };
}

/** Pick image files out of a drop, paste or picker. */
export function imageFiles(list: FileList | DataTransferItemList | File[] | null | undefined): File[] {
  if (!list) return [];
  const out: File[] = [];
  for (const item of Array.from(list as ArrayLike<File | DataTransferItem>)) {
    const f = item instanceof File ? item : item.kind === 'file' ? item.getAsFile() : null;
    if (f && (f.type.startsWith('image/') || /\.(jpe?g|png|webp|avif|gif|bmp|heic|heif)$/i.test(f.name))) out.push(f);
  }
  return out;
}
