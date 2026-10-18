// Eye detection with MediaPipe's Face Landmarker. Loaded on first use, from
// this site's own origin (the WASM runtime and the model are part of the
// build), so it keeps working offline once fetched.

import type { FaceLandmarker } from '@mediapipe/tasks-vision';

/** Iris centres in the 478-point mesh, then the eye corners as a fallback. */
const IRIS = [468, 473] as const;
const CORNERS = [
  [33, 133],
  [362, 263],
] as const;

let landmarker: Promise<FaceLandmarker> | null = null;

function load(): Promise<FaceLandmarker> {
  landmarker ??= (async () => {
    const { FaceLandmarker, FilesetResolver } = await import('@mediapipe/tasks-vision');
    const base = new URL('mediapipe', document.baseURI).href;
    const fileset = await FilesetResolver.forVisionTasks(base);
    return FaceLandmarker.createFromOptions(fileset, {
      baseOptions: { modelAssetPath: new URL('models/face_landmarker.task', document.baseURI).href, delegate: 'CPU' },
      runningMode: 'IMAGE',
      numFaces: 1,
    });
  })();
  landmarker.catch(() => (landmarker = null));
  return landmarker;
}

/** Warm the detector up without waiting for it. */
export function preloadFaces(): void {
  void load().catch(() => {});
}

/**
 * The two eye centres as `[x1, y1, x2, y2]`, normalised to the image, or
 * null when there is no face. The order is anatomical (the subject's right
 * eye first), so it matches between photos whatever their rotation.
 */
export async function findEyes(image: ImageBitmap): Promise<number[] | null> {
  const lm = await load();
  const result = lm.detect(image);
  const face = result.faceLandmarks?.[0];
  if (!face || face.length < 400) return null;
  const at = (i: number) => face[i];
  if (face.length >= 478) {
    const [r, l] = IRIS.map(at);
    return [r.x, r.y, l.x, l.y];
  }
  const [r, l] = CORNERS.map(([a, b]) => ({ x: (at(a).x + at(b).x) / 2, y: (at(a).y + at(b).y) / 2 }));
  return [r.x, r.y, l.x, l.y];
}
