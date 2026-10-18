// The preview: canvas sizing, the split handles, the eye overlay, and pointer,
// wheel and keyboard input. All geometry is the core's; this file only
// measures CSS pixels and forwards them.

import type { Scene } from './core';

export interface StageHost {
  scene: Scene;
  /** Which photo a gesture moves: a fixed choice, or whichever side it starts on. */
  target(): 'auto' | 0 | 1;
  touched(slot: 0 | 1): void;
  changed(): void;
  /** Eye positions to mark, per photo, in normalised displayed coordinates. */
  eyes(): (number[] | null)[];
  showEyes(): boolean;
}

interface Point {
  x: number;
  y: number;
}

const ROT_INSET = 30;
const SVG = 'http://www.w3.org/2000/svg';

export class Stage {
  readonly canvas = document.getElementById('canvas') as HTMLCanvasElement;
  private readonly wrap = document.getElementById('canvas-wrap')!;
  private readonly el = document.getElementById('stage')!;
  private readonly pos = document.getElementById('handle-pos')!;
  private readonly rot = document.getElementById('handle-rot')!;
  private readonly guide = document.getElementById('guide') as unknown as SVGLineElement;
  private readonly eyeLayer = document.getElementById('eyes') as unknown as SVGGElement;
  private pointers = new Map<number, Point>();
  private slot: 0 | 1 = 0;
  /** CSS size of the canvas and its backing-store size. */
  css = { w: 1, h: 1 };
  device = { w: 1, h: 1 };
  maxBacking = 4096;

  constructor(private host: StageHost) {
    this.bindHandles();
    this.bindCanvas();
  }

  replaceCanvas(): HTMLCanvasElement {
    // A canvas that has given out a WebGPU context cannot give a WebGL one.
    const fresh = this.canvas.cloneNode() as HTMLCanvasElement;
    this.canvas.replaceWith(fresh);
    (this as { canvas: HTMLCanvasElement }).canvas = fresh;
    this.bindCanvas();
    return fresh;
  }

  /** Fit the canvas to the stage at the frame's aspect ratio. */
  layout(aspect: number): boolean {
    const style = getComputedStyle(this.el);
    const padX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    const padY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    const availW = Math.max(40, this.el.clientWidth - padX);
    const availH = Math.max(40, this.el.clientHeight - padY);
    let w = availW;
    let h = w / aspect;
    if (h > availH) {
      h = availH;
      w = h * aspect;
    }
    w = Math.floor(w);
    h = Math.floor(h);
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    const k = Math.min(dpr, this.maxBacking / Math.max(w, h));
    const dw = Math.max(1, Math.round(w * k));
    const dh = Math.max(1, Math.round(h * k));
    const changed = w !== this.css.w || h !== this.css.h || dw !== this.device.w || dh !== this.device.h;
    if (changed) {
      this.css = { w, h };
      this.device = { w: dw, h: dh };
      this.canvas.style.width = `${w}px`;
      this.canvas.style.height = `${h}px`;
      this.wrap.style.width = `${w}px`;
      this.wrap.style.height = `${h}px`;
      this.host.scene.set_view(w, h);
    }
    return changed;
  }

  /** Move the handles, guide and eye markers to match the scene. */
  update(visible: boolean): void {
    const s = this.host.scene;
    const h = visible ? s.handles(ROT_INSET) : [];
    const show = h.length === 8;
    this.pos.hidden = !show;
    this.rot.hidden = !show;
    if (show) {
      this.pos.style.transform = `translate(${h[0]}px, ${h[1]}px)`;
      this.rot.style.transform = `translate(${h[2]}px, ${h[3]}px)`;
      this.guide.setAttribute('x1', String(h[4]));
      this.guide.setAttribute('y1', String(h[5]));
      this.guide.setAttribute('x2', String(h[6]));
      this.guide.setAttribute('y2', String(h[7]));
    }
    const angle = s.angle();
    const position = s.position() * 100;
    this.rot.setAttribute('aria-valuenow', angle.toFixed(1));
    this.rot.setAttribute('aria-valuetext', `${Math.round(angle)} degrees`);
    this.pos.setAttribute('aria-valuenow', position.toFixed(1));
    const p = Math.round(position);
    this.pos.setAttribute('aria-valuetext', p === 0 ? 'centre' : `${Math.abs(p)}% towards ${p > 0 ? 'B' : 'A'}`);
    this.drawEyes(visible);
  }

  private drawEyes(visible: boolean): void {
    this.eyeLayer.replaceChildren();
    if (!visible || !this.host.showEyes()) return;
    const eyes = this.host.eyes();
    for (const slot of [0, 1] as const) {
      const e = eyes[slot];
      if (!e || !this.host.scene.has_image(slot)) continue;
      for (const k of [0, 2]) {
        const [x, y] = this.host.scene.image_point_to_view(slot, e[k], e[k + 1]);
        const ring = document.createElementNS(SVG, 'circle');
        ring.setAttribute('cx', String(x));
        ring.setAttribute('cy', String(y));
        ring.setAttribute('r', slot === 0 ? '9' : '13');
        ring.setAttribute('class', slot === 0 ? 'eye-a' : 'eye-b');
        const dot = document.createElementNS(SVG, 'circle');
        dot.setAttribute('cx', String(x));
        dot.setAttribute('cy', String(y));
        dot.setAttribute('r', '1.8');
        dot.setAttribute('class', 'dot');
        this.eyeLayer.append(ring, dot);
      }
    }
  }

  private local(e: { clientX: number; clientY: number }): Point {
    const r = this.canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private interacting(on: boolean): void {
    this.el.classList.toggle('interacting', on);
  }

  private bindHandles(): void {
    const drag = (el: HTMLElement, start: () => void, move: (p: Point) => void, end: () => void) => {
      el.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        el.setPointerCapture(e.pointerId);
        el.classList.add('dragging');
        el.focus({ preventScroll: true });
        this.interacting(true);
        start();
      });
      el.addEventListener('pointermove', (e) => {
        if (!el.hasPointerCapture(e.pointerId)) return;
        move(this.local(e));
        this.host.changed();
      });
      const stop = (e: PointerEvent) => {
        if (!el.hasPointerCapture(e.pointerId)) return;
        el.releasePointerCapture(e.pointerId);
        el.classList.remove('dragging');
        this.interacting(false);
        end();
        this.host.changed();
      };
      el.addEventListener('pointerup', stop);
      el.addEventListener('pointercancel', stop);
    };
    const s = () => this.host.scene;
    drag(this.pos, () => {}, (p) => s().drag_position(p.x, p.y), () => {});
    drag(this.rot, () => s().begin_rotate(), (p) => s().drag_rotate(p.x, p.y), () => s().end_rotate());

    const keys = (el: HTMLElement, step: (delta: number, absolute?: number) => void) => {
      el.addEventListener('keydown', (e) => {
        const big = e.shiftKey ? 10 : 1;
        const map: Record<string, () => void> = {
          ArrowRight: () => step(big),
          ArrowUp: () => step(big),
          ArrowLeft: () => step(-big),
          ArrowDown: () => step(-big),
          PageUp: () => step(15),
          PageDown: () => step(-15),
          Home: () => step(0, -1),
          End: () => step(0, 1),
        };
        const f = map[e.key];
        if (!f) return;
        e.preventDefault();
        f();
        this.el.classList.add('show-guide');
        this.host.changed();
      });
      el.addEventListener('blur', () => this.el.classList.remove('show-guide'));
    };
    keys(this.rot, (d, abs) => s().set_angle(abs === undefined ? s().angle() + d : abs * 180));
    keys(this.pos, (d, abs) => s().set_position(abs === undefined ? s().position() + d / 100 : abs));
  }

  private bindCanvas(): void {
    const c = this.canvas;
    c.addEventListener('pointerdown', (e) => {
      if (!this.host.scene.has_image(0) && !this.host.scene.has_image(1)) return;
      c.setPointerCapture(e.pointerId);
      const p = this.local(e);
      if (this.pointers.size === 0) {
        const t = this.host.target();
        this.slot = t === 'auto' ? (this.host.scene.side_at(p.x, p.y) as 0 | 1) : t;
        this.host.touched(this.slot);
      }
      this.pointers.set(e.pointerId, p);
      c.classList.add('grabbing');
      this.interacting(true);
    });
    c.addEventListener('pointermove', (e) => {
      const prev = this.pointers.get(e.pointerId);
      if (!prev) return;
      const next = this.local(e);
      const s = this.host.scene;
      if (this.pointers.size === 1) {
        s.pan(this.slot, next.x - prev.x, next.y - prev.y);
      } else {
        // Two fingers: pan with the midpoint, zoom with the spread, turn
        // with the twist, all about the midpoint.
        const others = [...this.pointers.entries()].filter(([id]) => id !== e.pointerId);
        const o = others[0][1];
        const m0 = { x: (prev.x + o.x) / 2, y: (prev.y + o.y) / 2 };
        const m1 = { x: (next.x + o.x) / 2, y: (next.y + o.y) / 2 };
        const d0 = Math.hypot(prev.x - o.x, prev.y - o.y);
        const d1 = Math.hypot(next.x - o.x, next.y - o.y);
        const a0 = Math.atan2(prev.y - o.y, prev.x - o.x);
        const a1 = Math.atan2(next.y - o.y, next.x - o.x);
        s.pan(this.slot, m1.x - m0.x, m1.y - m0.y);
        if (d0 > 4 && d1 > 4) s.zoom_at(this.slot, d1 / d0, m1.x, m1.y);
        let da = a1 - a0;
        if (da > Math.PI) da -= 2 * Math.PI;
        if (da < -Math.PI) da += 2 * Math.PI;
        s.rotate_at(this.slot, da, m1.x, m1.y);
      }
      this.pointers.set(e.pointerId, next);
      this.host.changed();
    });
    const up = (e: PointerEvent) => {
      if (!this.pointers.delete(e.pointerId)) return;
      if (this.pointers.size === 0) {
        c.classList.remove('grabbing');
        this.interacting(false);
        this.host.changed();
      }
    };
    c.addEventListener('pointerup', up);
    c.addEventListener('pointercancel', up);
    c.addEventListener(
      'wheel',
      (e) => {
        if (!this.host.scene.has_image(0) && !this.host.scene.has_image(1)) return;
        e.preventDefault();
        const p = this.local(e);
        const t = this.host.target();
        const slot = t === 'auto' ? (this.host.scene.side_at(p.x, p.y) as 0 | 1) : t;
        this.host.touched(slot);
        // Trackpad pinches arrive as ctrl+wheel with small deltas.
        const rate = e.ctrlKey ? 0.01 : 0.0015;
        const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
        this.host.scene.zoom_at(slot, Math.exp(-dy * rate), p.x, p.y);
        this.host.changed();
      },
      { passive: false },
    );
  }
}
