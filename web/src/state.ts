// Settings in the URL hash: compact, human-readable and forgiving. Only the
// settings travel, never the photos, so a link reproduces a composition
// once the same two photos are loaded.
//
//   #s=28,0.0412&e=s,0.12&a=0,0,1,0&b=0.02,-0.1,1.31,4.5&p=story&c=srgb&r=3,0.6,1

export interface Placement {
  cx: number;
  cy: number;
  zoom: number;
  rot: number;
}

export type PresetId = 'ig-portrait' | 'ig-square' | 'story' | 'x' | 'source' | 'custom';

export interface Settings {
  angle: number;
  offset: number;
  soft: boolean;
  feather: number;
  a: Placement;
  b: Placement;
  preset: PresetId;
  custom: [number, number];
  color: 'p3' | 'srgb';
  sweep: number;
  hold: number;
  roundTrip: boolean;
}

export const PRESETS: Record<Exclude<PresetId, 'source' | 'custom'>, [number, number]> = {
  'ig-portrait': [1080, 1350],
  'ig-square': [1080, 1080],
  story: [1080, 1920],
  x: [1600, 900],
};

const identity = (): Placement => ({ cx: 0, cy: 0, zoom: 1, rot: 0 });

export function defaults(): Settings {
  return {
    angle: 28,
    offset: 0,
    soft: false,
    feather: 0.08,
    a: identity(),
    b: identity(),
    preset: 'ig-portrait',
    custom: [2000, 2000],
    color: 'p3',
    sweep: 3,
    hold: 0.6,
    roundTrip: true,
  };
}

/** Shortest decimal for `v` at `digits` places: 0.5000 -> "0.5", -0 -> "0". */
export function num(v: number, digits: number): string {
  const s = v.toFixed(digits).replace(/\.?0+$/, '');
  return s === '-0' ? '0' : s;
}

function placement(p: Placement): string {
  return [num(p.cx, 4), num(p.cy, 4), num(p.zoom, 4), num(p.rot, 2)].join(',');
}

const samePlacement = (p: Placement, q: Placement) => placement(p) === placement(q);

/** Serialise, leaving out anything still at its default. */
export function encode(s: Settings): string {
  const d = defaults();
  const parts: string[] = [];
  const add = (k: string, v: string, def: string) => {
    if (v !== def) parts.push(`${k}=${v}`);
  };
  add('s', `${num(s.angle, 2)},${num(s.offset, 4)}`, `${num(d.angle, 2)},${num(d.offset, 4)}`);
  add('e', `${s.soft ? 's' : 'h'},${num(s.feather, 3)}`, `h,${num(d.feather, 3)}`);
  if (!samePlacement(s.a, d.a)) parts.push(`a=${placement(s.a)}`);
  if (!samePlacement(s.b, d.b)) parts.push(`b=${placement(s.b)}`);
  const preset = s.preset === 'custom' ? `${s.custom[0]}x${s.custom[1]}` : s.preset;
  add('p', preset, d.preset);
  add('c', s.color, d.color);
  add('r', `${num(s.sweep, 2)},${num(s.hold, 2)},${s.roundTrip ? 1 : 0}`, `${num(d.sweep, 2)},${num(d.hold, 2)},1`);
  return parts.join('&');
}

function numbers(v: string | null, n: number): number[] | null {
  if (v == null) return null;
  const xs = v.split(',').map(Number);
  return xs.length === n && xs.every(Number.isFinite) ? xs : null;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function readPlacement(v: string | null): Placement | null {
  const xs = numbers(v, 4);
  if (!xs) return null;
  return { cx: clamp(xs[0], -4, 4), cy: clamp(xs[1], -4, 4), zoom: clamp(xs[2], 0.05, 40), rot: clamp(xs[3], -180, 180) };
}

/** Parse a hash (with or without the leading '#'). Anything malformed or
 * missing falls back to its default, so old or hand-edited links still open. */
export function decode(hash: string): Settings {
  const s = defaults();
  const q = new URLSearchParams(hash.replace(/^#/, ''));
  const split = numbers(q.get('s'), 2);
  if (split) {
    s.angle = clamp(split[0], -180, 180);
    s.offset = clamp(split[1], -4, 4);
  }
  const edge = q.get('e')?.split(',');
  if (edge && (edge[0] === 'h' || edge[0] === 's')) {
    s.soft = edge[0] === 's';
    const f = Number(edge[1]);
    if (Number.isFinite(f)) s.feather = clamp(f, 0, 0.5);
  }
  s.a = readPlacement(q.get('a')) ?? s.a;
  s.b = readPlacement(q.get('b')) ?? s.b;
  const p = q.get('p');
  const custom = p?.match(/^(\d{2,5})x(\d{2,5})$/);
  if (custom) {
    s.preset = 'custom';
    s.custom = [clamp(Number(custom[1]), 16, 16384), clamp(Number(custom[2]), 16, 16384)];
  } else if (p && (p in PRESETS || p === 'source')) {
    s.preset = p as PresetId;
  }
  const c = q.get('c');
  if (c === 'p3' || c === 'srgb') s.color = c;
  const r = numbers(q.get('r'), 3);
  if (r) {
    s.sweep = clamp(r[0], 1, 6);
    s.hold = clamp(r[1], 0, 2);
    s.roundTrip = r[2] !== 0;
  }
  return s;
}
