// diagonal blend: wiring. The DOM, input and browser APIs live here; the
// geometry, colour, file parsing and rendering live in the Rust core.

import { loadCore, type Core, type Renderer, type Scene } from './core';
import { deliver, detectSupport, renderReveal, renderStill, videoSize, type StillFormat, type Support, type VideoFormat } from './export';
import { findEyes, preloadFaces } from './faces';
import { decodePhoto, imageFiles, type Photo } from './images';
import { setupPwa, takeSharedPhotos } from './pwa';
import { Stage } from './stage';
import { PRESETS, decode, defaults, encode, type Settings } from './state';
import { toast } from './toast';

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const radios = (name: string) => [...document.querySelectorAll<HTMLInputElement>(`input[name="${name}"]`)];
const radio = (name: string) => radios(name).find((r) => r.checked)?.value ?? '';
const setRadio = (name: string, value: string) => radios(name).forEach((r) => (r.checked = r.value === value));

const settings: Settings = decode(location.hash);
/** Placements from the link wait for the first photo in each slot. */
const restored = [!same(settings.a, defaults().a), !same(settings.b, defaults().b)];
const photos: (Photo | null)[] = [null, null];
const eyes: (number[] | null)[] = [null, null];
const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');

let core: Core;
let scene: Scene;
let renderer: Renderer;
let stage: Stage;
let support: Support = { avif: false, webm: null, mp4: null };
let active: 0 | 1 = 0;
let revealTime: number | null = null;
let busy = false;
let bg = new Float64Array([0.9, 0.89, 0.86]);

function same(p: Settings['a'], q: Settings['a']) {
  return p.cx === q.cx && p.cy === q.cy && p.zoom === q.zoom && p.rot === q.rot;
}

// ---------------------------------------------------------------- theme

const THEMES = ['system', 'light', 'dark'] as const;
function applyTheme(t: (typeof THEMES)[number]) {
  if (t === 'system') delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = t;
  $('theme').setAttribute('aria-label', `Theme: ${t === 'system' ? 'match the system' : t}. Change theme`);
  try {
    if (t === 'system') localStorage.removeItem('theme');
    else localStorage.setItem('theme', t);
  } catch {
    // Private mode: the choice lasts for this visit only.
  }
  readBackground();
}
function currentTheme(): (typeof THEMES)[number] {
  const t = document.documentElement.dataset.theme;
  return t === 'light' || t === 'dark' ? t : 'system';
}
function readBackground() {
  const v = getComputedStyle(document.documentElement).getPropertyValue('--surface-2').trim();
  const m = v.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  if (m) bg = new Float64Array([1, 2, 3].map((i) => parseInt(m[i], 16) / 255));
  invalidate();
}

// ---------------------------------------------------------------- rendering

let frameRequested = false;
let hashTimer = 0;

function invalidate() {
  if (!scene || frameRequested) return;
  frameRequested = true;
  requestAnimationFrame(draw);
}

function draw() {
  frameRequested = false;
  if (stage.layout(scene.aspect())) renderer.resize(stage.device.w, stage.device.h);
  renderer.render(scene, bg, revealTime ?? -1);
  stage.update(revealTime === null && (photos[0] !== null || photos[1] !== null));
  syncControls();
}

/** Something in the scene changed: redraw, mirror it into the URL. */
function changed() {
  pull();
  invalidate();
  clearTimeout(hashTimer);
  hashTimer = window.setTimeout(writeHash, 250);
}

function writeHash() {
  const h = encode(settings);
  history.replaceState(null, '', h ? `#${h}` : location.pathname + location.search);
}

/** Scene -> settings. */
function pull() {
  settings.angle = scene.angle();
  settings.offset = scene.offset();
  settings.soft = scene.soft();
  settings.feather = scene.feather();
  for (const slot of [0, 1] as const) {
    const [cx, cy, zoom, rot] = scene.placement(slot);
    settings[slot === 0 ? 'a' : 'b'] = { cx, cy, zoom, rot };
  }
}

/** Settings -> scene. */
function push() {
  const [w, h] = outputSize();
  scene.set_frame(w, h);
  scene.set_angle(settings.angle);
  scene.set_offset(settings.offset);
  scene.set_soft(settings.soft);
  scene.set_feather(settings.feather);
  scene.set_reveal(settings.sweep, settings.hold, settings.roundTrip);
  for (const slot of [0, 1] as const) {
    const p = settings[slot === 0 ? 'a' : 'b'];
    scene.set_placement(slot, p.cx, p.cy, p.zoom, p.rot);
  }
}

function outputSize(): [number, number] {
  if (settings.preset === 'custom') return settings.custom;
  if (settings.preset === 'source') {
    const big = photos
      .filter((p): p is Photo => p !== null)
      .sort((p, q) => q.sourceWidth * q.sourceHeight - p.sourceWidth * p.sourceHeight)[0];
    return big ? [big.sourceWidth, big.sourceHeight] : PRESETS['ig-portrait'];
  }
  return PRESETS[settings.preset];
}

// ---------------------------------------------------------------- controls

function setOut(id: string, text: string) {
  $(`${id}-out`).textContent = text;
}

function adjustSlot(): 0 | 1 {
  const t = radio('target');
  return t === '0' ? 0 : t === '1' ? 1 : active;
}

/** Keep the panel's sliders in step with the scene (skipping the one in use). */
function syncControls() {
  const focused = document.activeElement;
  const set = (id: string, v: number, text: string) => {
    const el = $<HTMLInputElement>(id);
    if (el !== focused || !el.matches(':active')) el.value = String(v);
    setOut(id, text);
  };
  set('angle', settings.angle, `${settings.angle.toFixed(1)}°`);
  const pos = scene.position() * 100;
  set('position', pos, `${Math.round(pos)}%`);
  set('feather', settings.feather * 100, `${(settings.feather * 100).toFixed(1).replace(/\.0$/, '')}%`);
  $<HTMLInputElement>('feather').disabled = !settings.soft;
  setRadio('edge', settings.soft ? 'soft' : 'hard');

  const slot = adjustSlot();
  const [cx, cy, zoom, rot] = scene.placement(slot);
  set('zoom', Math.round(Math.log2(zoom) * 100), `${zoom.toFixed(2)}×`);
  set('rotate', rot, `${rot.toFixed(1)}°`);
  set('pan-x', cx * 100, `${Math.round(cx * 100)}`);
  set('pan-y', -cy * 100, `${Math.round(-cy * 100)}`);
  const has = photos[slot] !== null;
  $<HTMLFieldSetElement>('adjust-sliders').disabled = !has;
  $('adjust-note').textContent = has
    ? `Adjusting photo ${slot === 0 ? 'A' : 'B'}. Drag on either side of the line to move that photo; pinch or scroll to zoom it; twist with two fingers to turn it.`
    : 'Load a photo to move, zoom or turn it.';
}

function bindControls() {
  const range = (id: string, apply: (v: number) => void) =>
    $<HTMLInputElement>(id).addEventListener('input', (e) => {
      apply(Number((e.target as HTMLInputElement).value));
      changed();
    });
  range('angle', (v) => scene.set_angle(v));
  range('position', (v) => scene.set_position(v / 100));
  range('feather', (v) => scene.set_feather(v / 100));
  for (const r of radios('edge')) r.addEventListener('change', () => (scene.set_soft(r.value === 'soft'), changed()));

  const placement = (f: (p: Float64Array) => number[]) => {
    const slot = adjustSlot();
    const [cx, cy, zoom, rot] = f(scene.placement(slot));
    scene.set_placement(slot, cx, cy, zoom, rot);
  };
  range('zoom', (v) => placement(([cx, cy, , rot]) => [cx, cy, 2 ** (v / 100), rot]));
  range('rotate', (v) => placement(([cx, cy, zoom]) => [cx, cy, zoom, v]));
  range('pan-x', (v) => placement(([, cy, zoom, rot]) => [v / 100, cy, zoom, rot]));
  range('pan-y', (v) => placement(([cx, , zoom, rot]) => [cx, -v / 100, zoom, rot]));
  $('reset-photo').addEventListener('click', () => (scene.reset_placement(adjustSlot()), changed()));
  for (const r of radios('target')) r.addEventListener('change', () => syncControls());

  $('swap').addEventListener('click', () => {
    scene.swap();
    renderer.swap();
    photos.reverse();
    eyes.reverse();
    showSlots();
    changed();
  });

  $('align').addEventListener('click', alignFaces);
  $('show-eyes').addEventListener('change', invalidate);

  const preset = $<HTMLSelectElement>('preset');
  preset.value = settings.preset;
  $('custom-size').hidden = settings.preset !== 'custom';
  $<HTMLInputElement>('custom-w').value = String(settings.custom[0]);
  $<HTMLInputElement>('custom-h').value = String(settings.custom[1]);
  preset.addEventListener('change', () => {
    settings.preset = preset.value as Settings['preset'];
    $('custom-size').hidden = settings.preset !== 'custom';
    applyFrame();
  });
  for (const id of ['custom-w', 'custom-h']) {
    $(id).addEventListener('change', () => {
      const w = Math.round(Math.min(16384, Math.max(16, Number($<HTMLInputElement>('custom-w').value) || 2000)));
      const h = Math.round(Math.min(16384, Math.max(16, Number($<HTMLInputElement>('custom-h').value) || 2000)));
      settings.custom = [w, h];
      applyFrame();
    });
  }
  const color = $<HTMLSelectElement>('color');
  color.value = settings.color;
  color.addEventListener('change', () => ((settings.color = color.value as Settings['color']), changed()));

  const reveal = () => {
    settings.sweep = Number($<HTMLInputElement>('sweep').value);
    settings.hold = Number($<HTMLInputElement>('hold').value);
    settings.roundTrip = $<HTMLInputElement>('round-trip').checked;
    scene.set_reveal(settings.sweep, settings.hold, settings.roundTrip);
    setOut('sweep', `${settings.sweep.toFixed(1)} s`);
    setOut('hold', `${settings.hold.toFixed(1)} s`);
    changed();
  };
  $<HTMLInputElement>('sweep').value = String(settings.sweep);
  $<HTMLInputElement>('hold').value = String(settings.hold);
  $<HTMLInputElement>('round-trip').checked = settings.roundTrip;
  for (const id of ['sweep', 'hold']) $(id).addEventListener('input', reveal);
  $('round-trip').addEventListener('change', reveal);
  setOut('sweep', `${settings.sweep.toFixed(1)} s`);
  setOut('hold', `${settings.hold.toFixed(1)} s`);

  $('preview-reveal').addEventListener('click', previewReveal);
  $('save-still').addEventListener('click', saveStill);
  $('save-video').addEventListener('click', saveVideo);
  reducedMotion.addEventListener('change', updateButtons);
}

function applyFrame() {
  const [w, h] = outputSize();
  scene.set_frame(w, h);
  changed();
}

function updateButtons() {
  const both = photos[0] !== null && photos[1] !== null;
  $<HTMLButtonElement>('align').disabled = !both || busy;
  $<HTMLButtonElement>('swap').disabled = !(photos[0] || photos[1]) || busy;
  $<HTMLButtonElement>('save-still').disabled = !both || busy;
  $<HTMLButtonElement>('save-video').disabled = !both || busy || !(support.webm || support.mp4);
  const preview = $<HTMLButtonElement>('preview-reveal');
  preview.disabled = !both || busy || reducedMotion.matches;
  preview.title = reducedMotion.matches ? 'Off because your system asks for reduced motion. Export still works.' : '';
}

function showSlots() {
  for (const slot of [0, 1] as const) {
    const p = photos[slot];
    const img = $<HTMLImageElement>(`thumb-${slot}`);
    img.hidden = !p;
    if (p) {
      img.src = p.url;
      img.alt = `Photo ${slot === 0 ? 'A' : 'B'}: ${p.name}`;
    }
    $(`name-${slot}`).textContent = p ? p.name : 'No photo yet';
    $(`info-${slot}`).textContent = p
      ? [`${p.sourceWidth}×${p.sourceHeight}`, p.profile === 'other' ? 'converted to sRGB' : p.profile === 'untagged' ? 'sRGB, untagged' : p.profile, p.orientation !== 1 ? `EXIF ${p.orientation}` : '']
          .filter(Boolean)
          .join(' · ')
      : '';
  }
  const any = photos[0] !== null || photos[1] !== null;
  $('empty').hidden = any;
  $('stage').classList.toggle('empty-state', !any);
  document.body.dataset.photos = String(photos.filter(Boolean).length);
  if (settings.preset === 'source') applyFrame();
  updateButtons();
}

// ---------------------------------------------------------------- photos

async function loadInto(slot: 0 | 1, file: Blob, name: string) {
  $(`name-${slot}`).textContent = 'Opening…';
  document.body.dataset.state = 'loading';
  try {
    const photo = await decodePhoto(core, file, name, renderer.max_texture_size());
    renderer.set_texture(slot, new Uint8Array(photo.rgba.buffer), photo.width, photo.height);
    photo.rgba = new Uint8ClampedArray(0);
    scene.set_image(slot, photo.displayWidth, photo.displayHeight, photo.residual, photo.space);
    if (!restored[slot]) scene.reset_placement(slot);
    restored[slot] = false;
    if (photos[slot]) URL.revokeObjectURL(photos[slot]!.url);
    photos[slot] = photo;
    eyes[slot] = null;
    active = slot;
    if (photos[0] && photos[1]) preloadFaces();
  } catch (e) {
    toast((e as Error).message || `Could not open ${name}.`, 'warn');
  } finally {
    document.body.dataset.state = 'ready';
    showSlots();
    changed();
  }
}

/** Two files fill both slots; one fills the empty slot, or `prefer`. */
async function loadFiles(files: File[], prefer?: 0 | 1) {
  if (files.length === 0) return toast('Those were not image files.', 'warn');
  if (files.length > 2) toast('Using the first two photos.');
  if (files.length >= 2) {
    await Promise.all([loadInto(0, files[0], files[0].name), loadInto(1, files[1], files[1].name)]);
    return;
  }
  const slot = prefer ?? (photos[0] === null ? 0 : photos[1] === null ? 1 : active);
  await loadInto(slot, files[0], files[0].name);
}

function bindLoading() {
  const pick = (input: HTMLInputElement, slot?: 0 | 1) =>
    input.addEventListener('change', () => {
      const files = imageFiles(input.files);
      input.value = '';
      void loadFiles(slot === undefined ? files : files.slice(0, 1), slot);
    });
  pick($('pick-both'));
  for (const slot of [0, 1] as const) {
    pick($(`file-${slot}`), slot);
    pick($(`camera-${slot}`), slot);
  }

  const stageEl = $('stage');
  let depth = 0;
  const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes('Files');
  document.addEventListener('dragenter', (e) => {
    if (!hasFiles(e)) return;
    depth++;
    stageEl.classList.add('dragover');
  });
  document.addEventListener('dragleave', () => {
    if (--depth <= 0) stageEl.classList.remove('dragover');
  });
  document.addEventListener('dragover', (e) => {
    if (hasFiles(e)) e.preventDefault();
  });
  document.addEventListener('drop', (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0;
    stageEl.classList.remove('dragover');
    const files = imageFiles(e.dataTransfer?.files);
    let prefer: 0 | 1 | undefined;
    // One photo dropped on the preview replaces the side it lands on.
    if (files.length === 1 && photos[0] && photos[1] && e.target === stage.canvas) {
      const r = stage.canvas.getBoundingClientRect();
      prefer = scene.side_at(e.clientX - r.left, e.clientY - r.top) as 0 | 1;
    }
    void loadFiles(files, prefer);
  });
  document.addEventListener('paste', (e) => {
    const files = imageFiles(e.clipboardData?.items);
    if (files.length) {
      e.preventDefault();
      void loadFiles(files);
    }
  });

  $('samples').addEventListener('click', async () => {
    try {
      const names = ['scott-kelly.jpg', 'mark-kelly.jpg'];
      const files = await Promise.all(
        names.map(async (n) => new File([await (await fetch(`/samples/${n}`)).blob()], n, { type: 'image/jpeg' })),
      );
      await loadFiles(files);
      toast('Scott and Mark Kelly, identical twins and astronauts. Now tap Align faces.', 'info', 6000);
    } catch {
      toast('The sample photos are not available offline yet.', 'warn');
    }
  });
}

// ---------------------------------------------------------------- faces

async function eyesFor(slot: 0 | 1): Promise<number[] | null> {
  if (eyes[slot]) return eyes[slot];
  const p = photos[slot]!;
  let e = await findEyes(p.small);
  if (e && p.residual !== 1) {
    e = [...scene.stored_to_displayed(slot, e[0], e[1]), ...scene.stored_to_displayed(slot, e[2], e[3])];
  }
  eyes[slot] = e;
  return e;
}

async function alignFaces() {
  const btn = $<HTMLButtonElement>('align');
  const label = btn.lastChild!;
  const text = label.textContent;
  busy = true;
  updateButtons();
  label.textContent = ' Finding faces…';
  try {
    const [a, b] = [await eyesFor(0), await eyesFor(1)];
    if (!a || !b) {
      toast(`No face found in photo ${!a && !b ? 'A or B' : !a ? 'A' : 'B'}. Nothing was changed.`, 'warn');
    } else if (!scene.align(new Float64Array(a), new Float64Array(b))) {
      toast('Those eyes are too close together to line up.', 'warn');
    } else {
      $<HTMLInputElement>('show-eyes').checked = true;
      toast('Faces aligned: B is scaled, turned and moved onto A, split down the middle.');
    }
  } catch (e) {
    console.error(e);
    toast('The face detector could not start in this browser.', 'warn');
  } finally {
    label.textContent = text;
    busy = false;
    updateButtons();
    changed();
  }
}

// ---------------------------------------------------------------- export

const spaceCode = () => (settings.color === 'p3' ? 1 : 0) as 0 | 1;

function checkSize(w: number, h: number): boolean {
  if (w * h > 268_000_000) {
    toast(`${w} × ${h} is more than this tool will render. Try a smaller size.`, 'warn');
    return false;
  }
  return true;
}

async function saveStill() {
  const [w, h] = outputSize();
  if (!checkSize(w, h)) return;
  const format = radio('still') as StillFormat;
  busy = true;
  updateButtons();
  const btn = $<HTMLButtonElement>('save-still');
  btn.textContent = 'Rendering…';
  try {
    const started = performance.now();
    const blob = await renderStill(core, renderer, scene, format, w, h, spaceCode());
    await deliver(blob, `diagonal-blend-${w}x${h}.${format}`);
    const secs = ((performance.now() - started) / 1000).toFixed(1);
    toast(`Saved ${w} × ${h} ${format.toUpperCase()}, ${settings.color === 'p3' ? 'Display P3' : 'sRGB'}, ${(blob.size / 1e6).toFixed(1)} MB in ${secs} s.`);
  } catch (e) {
    console.error(e);
    toast(`Export failed: ${(e as Error).message}`, 'warn');
  } finally {
    btn.textContent = 'Save image';
    busy = false;
    updateButtons();
  }
}

async function saveVideo() {
  const format = radio('video') as VideoFormat;
  const controller = new AbortController();
  const bar = $<HTMLProgressElement>('progress-bar');
  const text = $('progress-text');
  $('progress').hidden = false;
  const cancel = () => controller.abort();
  $('cancel').addEventListener('click', cancel, { once: true });
  busy = true;
  updateButtons();
  const [w, h] = videoSize(...outputSize());
  try {
    const blob = await renderReveal(renderer, scene, format, support, [w, h], spaceCode(), (done, total) => {
      bar.value = done / total;
      text.textContent = `Frame ${done} of ${total}`;
    }, controller.signal);
    await deliver(blob, `diagonal-blend-reveal-${w}x${h}.${format}`);
    toast(`Saved a ${scene.reveal_duration().toFixed(1)} s ${format.toUpperCase()} at ${w} × ${h}.`);
  } catch (e) {
    if ((e as DOMException).name === 'AbortError') toast('Video export cancelled.');
    else {
      console.error(e);
      toast(`Video export failed: ${(e as Error).message}`, 'warn');
    }
  } finally {
    $('cancel').removeEventListener('click', cancel);
    $('progress').hidden = true;
    bar.value = 0;
    busy = false;
    updateButtons();
  }
}

function previewReveal() {
  if (reducedMotion.matches) return;
  const start = performance.now();
  const total = scene.reveal_duration();
  const step = (now: number) => {
    const t = (now - start) / 1000;
    if (t > total) {
      revealTime = null;
    } else {
      revealTime = t;
      requestAnimationFrame(step);
    }
    invalidate();
  };
  requestAnimationFrame(step);
}

function showFormats() {
  const avif = document.querySelector<HTMLElement>('[data-format="avif"]')!;
  avif.hidden = !support.avif;
  for (const f of ['webm', 'mp4'] as const) {
    document.querySelector<HTMLElement>(`[data-format="${f}"]`)!.hidden = !support[f];
  }
  const firstVideo = support.webm ? 'webm' : support.mp4 ? 'mp4' : '';
  if (firstVideo) setRadio('video', firstVideo);
  else $('video-formats').insertAdjacentHTML('afterend', '<p class="note">This browser cannot encode video (WebCodecs is missing).</p>');
}

// ---------------------------------------------------------------- boot

async function createRenderer(): Promise<Renderer> {
  const params = new URLSearchParams(location.search);
  const forceWebgl = params.get('renderer') === 'webgl2';
  const wantP3 = matchMedia('(color-gamut: p3)').matches;
  try {
    return await core.Renderer.create(stage.canvas, forceWebgl, wantP3);
  } catch (e) {
    if (forceWebgl) throw e;
    // WebGPU was there but would not start: try again on a fresh canvas.
    console.warn('WebGPU failed, falling back to WebGL2', e);
    return core.Renderer.create(stage.replaceCanvas(), true, wantP3);
  }
}

async function boot() {
  applyTheme(currentTheme());
  $('theme').addEventListener('click', () => applyTheme(THEMES[(THEMES.indexOf(currentTheme()) + 1) % THEMES.length]));
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', readBackground);
  setupPwa();

  core = await loadCore();
  scene = new core.Scene();
  stage = new Stage({
    scene,
    target: () => {
      const t = radio('target');
      return t === '0' ? 0 : t === '1' ? 1 : 'auto';
    },
    touched: (slot) => {
      if (active !== slot) {
        active = slot;
        syncControls();
      }
    },
    changed,
    eyes: () => eyes,
    showEyes: () => $<HTMLInputElement>('show-eyes').checked,
  });
  push();
  try {
    renderer = await createRenderer();
  } catch (e) {
    console.error(e);
    $('tech').textContent = 'No WebGPU or WebGL2 here, so nothing can be drawn.';
    document.querySelector('.drop h1')!.textContent = 'This browser cannot run diagonal blend.';
    document.querySelector('.drop p')!.textContent = 'It needs WebGPU or WebGL2. Try a current Chrome, Edge, Firefox or Safari.';
    return;
  }
  stage.maxBacking = Math.min(renderer.max_texture_size(), 4096);
  const backend = renderer.backend();
  const space = renderer.color_space();
  document.body.dataset.backend = backend;
  document.body.dataset.colorSpace = space;
  $('tech').textContent = `${backend === 'webgpu' ? 'WebGPU' : 'WebGL2'} · preview in ${space === 'display-p3' ? 'Display P3' : 'sRGB'} · Rust core via WebAssembly`;

  bindControls();
  bindLoading();
  new ResizeObserver(invalidate).observe($('stage'));
  readBackground();
  showSlots();
  document.body.dataset.state = 'ready';

  support = await detectSupport();
  showFormats();
  updateButtons();

  const shared = await takeSharedPhotos();
  if (shared.length) await loadFiles(shared);
}

void boot();
