// Exports. Every pixel comes from the core's offscreen render at the exact
// output size, never from the on-screen canvas.

import type { Core, Renderer, Scene } from './core';

export type StillFormat = 'png' | 'webp' | 'avif';
export type VideoFormat = 'webm' | 'mp4';

export interface Support {
  avif: boolean;
  webm: VideoCodec | null;
  mp4: VideoCodec | null;
}

interface VideoCodec {
  /** WebCodecs codec string, picked per size by `pick`. */
  pick(width: number, height: number): string;
  /** The muxer's name for it. */
  mux: string;
}

const FPS = 30;
/** Videos are capped at this long side; stills are not. */
export const VIDEO_MAX = 1920;
/** Background behind any part of the frame a photo does not reach. */
const BLACK = new Float64Array(3);

async function encoderSupports(codec: string, width = 1080, height = 1350): Promise<boolean> {
  if (typeof VideoEncoder === 'undefined') return false;
  try {
    const r = await VideoEncoder.isConfigSupported({ codec, width, height, bitrate: 6e6, framerate: FPS });
    return r.supported === true;
  } catch {
    return false;
  }
}

/** H.264 level from the frame size: 4.0 up to 1080p, 5.1 beyond. */
const avc = (w: number, h: number) => (Math.ceil(w / 16) * Math.ceil(h / 16) > 8192 ? 'avc1.640033' : 'avc1.640028');

export async function detectSupport(): Promise<Support> {
  let avif = false;
  try {
    const blob = await new OffscreenCanvas(2, 2).convertToBlob({ type: 'image/avif' });
    avif = blob.type === 'image/avif';
  } catch {
    avif = false;
  }
  let webm: VideoCodec | null = null;
  for (const [codec, mux] of [
    ['vp09.00.40.08', 'V_VP9'],
    ['av01.0.08M.08', 'V_AV1'],
  ] as const) {
    if (await encoderSupports(codec)) {
      webm = { pick: () => codec, mux };
      break;
    }
  }
  const mp4 = (await encoderSupports(avc(1080, 1350))) ? { pick: avc, mux: 'avc' } : null;
  return { avif, webm, mp4 };
}

export async function renderStill(
  core: Core,
  renderer: Renderer,
  scene: Scene,
  format: StillFormat,
  width: number,
  height: number,
  space: 0 | 1,
): Promise<Blob> {
  const px: Uint8Array = await renderer.render_pixels(scene, width, height, space, BLACK, -1);
  if (format === 'png') {
    return new Blob([core.encode_png(px, width, height, space) as Uint8Array<ArrayBuffer>], { type: 'image/png' });
  }
  const colorSpace: PredefinedColorSpace = space === 1 ? 'display-p3' : 'srgb';
  const canvas = new OffscreenCanvas(width, height);
  const ctx = canvas.getContext('2d', { colorSpace });
  if (!ctx) throw new Error('2D canvas unavailable');
  // Values are already in the target space; tagging the ImageData with the
  // same space means the browser copies them through unchanged.
  ctx.putImageData(new ImageData(new Uint8ClampedArray(px.buffer as ArrayBuffer), width, height, { colorSpace }), 0, 0);
  const type = format === 'webp' ? 'image/webp' : 'image/avif';
  const blob = await canvas.convertToBlob({ type, quality: 0.92 });
  if (format === 'webp') {
    const bytes = new Uint8Array(await blob.arrayBuffer());
    return new Blob([core.tag_webp(bytes, space) as Uint8Array<ArrayBuffer>], { type });
  }
  return blob;
}

export interface Progress {
  (done: number, total: number): void;
}

/** Fit a size inside `max` on its long side, keeping both sides even. */
export function videoSize(w: number, h: number, max = VIDEO_MAX): [number, number] {
  const k = Math.min(1, max / Math.max(w, h));
  const even = (v: number) => Math.max(2, Math.round((v * k) / 2) * 2);
  return [even(w), even(h)];
}

export async function renderReveal(
  core: Core,
  renderer: Renderer,
  scene: Scene,
  format: VideoFormat,
  support: Support,
  size: [number, number],
  space: 0 | 1,
  progress: Progress,
  signal: AbortSignal,
): Promise<Blob> {
  const codec = format === 'webm' ? support.webm : support.mp4;
  if (!codec) throw new Error(`${format.toUpperCase()} encoding is not available in this browser`);
  const [width, height] = videoSize(...size);
  const frames = Math.max(1, Math.round(scene.reveal_duration() * FPS));

  // The muxers are only fetched when a video is actually made.
  const { ArrayBufferTarget: WebmTarget, Muxer: WebmMuxer } = await import('webm-muxer');
  const { ArrayBufferTarget: Mp4Target, Muxer: Mp4Muxer } = await import('mp4-muxer');
  const webm = format === 'webm' ? new WebmMuxer({ target: new WebmTarget(), video: { codec: codec.mux, width, height, frameRate: FPS } }) : null;
  const mp4 =
    format === 'mp4'
      ? new Mp4Muxer({ target: new Mp4Target(), video: { codec: 'avc', width, height, frameRate: FPS }, fastStart: 'in-memory' })
      : null;

  let failure: unknown = null;
  // webm-muxer has no code for P3 primaries: let it write BT.709 and put
  // the right value in afterwards.
  let p3Webm = false;
  const encoder = new VideoEncoder({
    output: (chunk, meta) => {
      if (mp4) return mp4.addVideoChunk(chunk, meta);
      const cs = meta?.decoderConfig?.colorSpace;
      if (meta?.decoderConfig && (cs?.primaries as string | undefined) === 'smpte432') {
        p3Webm = true;
        meta = { ...meta, decoderConfig: { ...meta.decoderConfig, colorSpace: { ...cs, primaries: 'bt709' } } };
      }
      webm!.addVideoChunk(chunk, meta);
    },
    error: (e) => (failure = e),
  });
  encoder.configure({
    codec: codec.pick(width, height),
    width,
    height,
    framerate: FPS,
    bitrate: Math.round(width * height * FPS * 0.14),
    latencyMode: 'quality',
    ...(format === 'mp4' ? { avc: { format: 'avc' } } : {}),
  });

  // The pixels are sRGB-encoded in either set of primaries, full range.
  const colorSpace: VideoColorSpaceInit = {
    // 'smpte432' (P3 D65) is in WebCodecs but not yet in TypeScript's DOM types.
    primaries: (space === 1 ? 'smpte432' : 'bt709') as VideoColorPrimaries,
    transfer: 'iec61966-2-1',
    matrix: 'rgb',
    fullRange: true,
  };

  try {
    for (let i = 0; i < frames; i++) {
      if (signal.aborted) throw new DOMException('Export cancelled', 'AbortError');
      if (failure) throw failure;
      const t = i / FPS;
      const px: Uint8Array = await renderer.render_pixels(scene, width, height, space, BLACK, t);
      const frame = new VideoFrame(px, {
        format: 'RGBA',
        codedWidth: width,
        codedHeight: height,
        timestamp: Math.round((i * 1e6) / FPS),
        duration: Math.round(1e6 / FPS),
        colorSpace,
      });
      encoder.encode(frame, { keyFrame: i % (FPS * 2) === 0 });
      frame.close();
      while (encoder.encodeQueueSize > 4) {
        await new Promise((r) => encoder.addEventListener('dequeue', r, { once: true }));
      }
      progress(i + 1, frames);
    }
    await encoder.flush();
    if (failure) throw failure;
  } finally {
    if (encoder.state !== 'closed') encoder.close();
  }

  if (webm) {
    webm.finalize();
    const bytes = new Uint8Array(webm.target.buffer);
    if (p3Webm) core.webm_set_primaries(bytes, 12);
    return new Blob([bytes], { type: 'video/webm' });
  }
  mp4!.finalize();
  return new Blob([mp4!.target.buffer], { type: 'video/mp4' });
}

/** Hand a file to the user: the share sheet on phones, a download elsewhere. */
export async function deliver(blob: Blob, name: string): Promise<void> {
  const file = new File([blob], name, { type: blob.type });
  const coarse = matchMedia('(pointer: coarse)').matches;
  if (coarse && navigator.canShare?.({ files: [file] })) {
    try {
      await navigator.share({ files: [file] });
      return;
    } catch (e) {
      if ((e as DOMException).name === 'AbortError') return;
    }
  }
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
