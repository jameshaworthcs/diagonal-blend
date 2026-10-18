import { describe, expect, it } from 'vitest';
import { decode, defaults, encode, num, type Settings } from './state';

const busy = (): Settings => ({
  angle: -33.25,
  offset: 0.0412,
  soft: true,
  feather: 0.125,
  a: { cx: 0.1, cy: -0.25, zoom: 1.5, rot: 12.5 },
  b: { cx: -0.0213, cy: 0.3, zoom: 0.8125, rot: -90 },
  preset: 'story',
  custom: [2000, 2000],
  color: 'srgb',
  sweep: 4.5,
  hold: 1.2,
  roundTrip: false,
});

describe('url state', () => {
  it('round-trips every setting', () => {
    const s = busy();
    expect(decode(encode(s))).toEqual(s);
    expect(decode('#' + encode(s))).toEqual(s);
  });

  it('writes nothing for the defaults', () => {
    expect(encode(defaults())).toBe('');
    expect(decode('')).toEqual(defaults());
  });

  it('is compact', () => {
    const hash = encode(busy());
    expect(hash.length).toBeLessThan(110);
    expect(hash).toBe('s=-33.25,0.0412&e=s,0.125&a=0.1,-0.25,1.5,12.5&b=-0.0213,0.3,0.8125,-90&p=story&c=srgb&r=4.5,1.2,0');
  });

  it('round-trips a custom size', () => {
    const s = { ...defaults(), preset: 'custom' as const, custom: [3000, 1688] as [number, number] };
    expect(encode(s)).toBe('p=3000x1688');
    expect(decode(encode(s))).toEqual(s);
  });

  it('keeps defaults for malformed parts and clamps the rest', () => {
    const s = decode('#s=abc&e=x,1&a=1,2,3&b=9,-9,99,720&p=nonsense&c=cmyk&r=60,-1,1&zzz=1');
    const d = defaults();
    expect(s.angle).toBe(d.angle);
    expect(s.soft).toBe(false);
    expect(s.a).toEqual(d.a);
    expect(s.b).toEqual({ cx: 4, cy: -4, zoom: 40, rot: 180 });
    expect(s.preset).toBe(d.preset);
    expect(s.color).toBe(d.color);
    expect([s.sweep, s.hold]).toEqual([6, 0]);
  });

  it('rounds to the stored precision', () => {
    const s = { ...defaults(), angle: 10.123456, offset: 0.123456789 };
    const back = decode(encode(s));
    expect(back.angle).toBe(10.12);
    expect(back.offset).toBe(0.1235);
  });

  it('formats numbers tersely', () => {
    expect(num(0.5, 4)).toBe('0.5');
    expect(num(-0.00001, 4)).toBe('0');
    expect(num(12, 2)).toBe('12');
    expect(num(-90, 2)).toBe('-90');
  });
});
