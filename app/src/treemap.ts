// Squarified treemap with cushion shading, drawn to canvas once per layout.
// Hover and selection are painted on a separate overlay canvas.

import type { Slice } from "./api";
import { color, kindOf } from "./kinds";

export interface Rect { i: number; x: number; y: number; w: number; h: number; depth: number; label: boolean; text?: string; chain?: boolean }

const HEADER = 17;
const PAD = 2;

export class Treemap {
  private base: HTMLCanvasElement;
  private over: HTMLCanvasElement;
  private data: Slice | null = null;
  private kids: number[][] = [];
  rects: Rect[] = [];
  private hover = -1;
  private selected = -1;
  private dpr = 1;
  private w = 0;
  private h = 0;
  onHover: (i: number, x: number, y: number) => void = () => {};
  onSelect: (i: number) => void = () => {};
  onOpen: (i: number) => void = () => {};

  constructor(private host: HTMLElement) {
    this.base = document.createElement("canvas");
    this.over = document.createElement("canvas");
    host.prepend(this.over);
    host.prepend(this.base);
    new ResizeObserver(() => this.resize()).observe(host);
    this.over.addEventListener("mousemove", (e) => {
      const i = this.hit(e.offsetX, e.offsetY);
      if (i !== this.hover) {
        this.hover = i;
        this.paintOverlay();
      }
      this.onHover(i, e.offsetX, e.offsetY);
    });
    this.over.addEventListener("mouseleave", () => {
      this.hover = -1;
      this.paintOverlay();
      this.onHover(-1, 0, 0);
    });
    this.over.addEventListener("click", (e) => {
      const i = this.hit(e.offsetX, e.offsetY);
      if (i >= 0) this.onSelect(i);
    });
    this.over.addEventListener("dblclick", (e) => {
      const i = this.hit(e.offsetX, e.offsetY);
      if (i < 0 || !this.data) return;
      // Open the folder under the cursor (or the file's folder).
      const target = this.data.dir[i] ? i : this.data.parent[i];
      if (target > 0) this.onOpen(target);
    });
  }

  set(data: Slice) {
    this.data = data;
    this.kids = data.id.map(() => []);
    for (let i = 1; i < data.id.length; i++) this.kids[data.parent[i]].push(i);
    this.selected = -1;
    this.hover = -1;
    this.layout();
  }

  select(i: number) {
    this.selected = i;
    this.paintOverlay();
  }

  rectOf(i: number) {
    return this.rects.find((r) => r.i === i);
  }

  /** Zoom-in transition: the clicked rect grows to fill the canvas, then the new map is drawn. */
  async zoomFrom(r: { x: number; y: number; w: number; h: number } | undefined, reverse = false) {
    if (!r || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const snap = document.createElement("canvas");
    snap.width = this.base.width;
    snap.height = this.base.height;
    snap.getContext("2d")!.drawImage(this.base, 0, 0);
    const ctx = this.base.getContext("2d")!;
    const start = performance.now();
    const dur = 220;
    await new Promise<void>((done) => {
      const step = (t: number) => {
        let k = Math.min(1, (t - start) / dur);
        k = 1 - Math.pow(1 - k, 4); // strong ease-out
        if (reverse) k = 1 - k;
        const sx = r.x + (0 - r.x) * k, sy = r.y + (0 - r.y) * k;
        const sw = r.w + (this.w - r.w) * k, sh = r.h + (this.h - r.h) * k;
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, this.base.width, this.base.height);
        if (!reverse) {
          // grow the source rect region to full size
          const d = this.dpr;
          ctx.globalAlpha = 1;
          ctx.drawImage(snap, r.x * d, r.y * d, r.w * d, r.h * d, sx * d, sy * d, sw * d, sh * d);
        } else {
          const d = this.dpr;
          ctx.drawImage(snap, 0, 0, snap.width, snap.height, sx * d, sy * d, sw * d, sh * d);
        }
        if ((reverse ? 1 - k : k) < 1) requestAnimationFrame(step);
        else done();
      };
      requestAnimationFrame(step);
    });
  }

  private resize() {
    const r = this.host.getBoundingClientRect();
    this.dpr = window.devicePixelRatio || 1;
    this.w = Math.max(1, Math.floor(r.width));
    this.h = Math.max(1, Math.floor(r.height));
    for (const c of [this.base, this.over]) {
      c.width = Math.round(this.w * this.dpr);
      c.height = Math.round(this.h * this.dpr);
    }
    this.layout();
  }

  private layout() {
    this.rects = [];
    if (!this.data || this.w < 4) return this.paint();
    this.place(0, 0, 0, this.w, this.h, 0);
    this.paint();
  }

  /** Lay out the children of slice node `i` inside the given box. */
  private place(i: number, x: number, y: number, w: number, h: number, depth: number, prefix = "") {
    const d = this.data!;
    const kids = this.kids[i];
    if (!kids.length || w < 3 || h < 3) return;
    const total = d.size[i];
    if (total <= 0) return;
    // Scale so the kids fill only their share of the parent (rest = small items).
    const kidSum = kids.reduce((s, k) => s + d.size[k], 0);
    const area = w * h * Math.min(1, kidSum / total);
    const items = kids.map((k) => ({ k, a: (d.size[k] / kidSum) * area })).filter((o) => o.a > 0.5);
    squarify(items, x, y, w, h, (k, rx, ry, rw, rh) => {
      const isDir = d.dir[k] === 1 && this.kids[k].length > 0;
      const text = prefix ? `${prefix} › ${d.name[k]}` : d.name[k];
      // A folder that is just a wrapper around one folder hands its name down instead of a header.
      const only = isDir && this.kids[k].length === 1 && d.dir[this.kids[k][0]] === 1 && d.size[this.kids[k][0]] >= d.size[k] * 0.97;
      const label = isDir && !only && rw > 84 && rh > 46;
      this.rects.push({ i: k, x: rx, y: ry, w: rw, h: rh, depth, label, text, chain: only });
      if (only) this.place(k, rx, ry, rw, rh, depth, text);
      else if (isDir) {
        const top = label ? HEADER : PAD;
        this.place(k, rx + PAD, ry + top, rw - PAD * 2, rh - top - PAD, depth + 1);
      }
    });
  }

  private paint() {
    const ctx = this.base.getContext("2d")!;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.clearRect(0, 0, this.w, this.h);
    const d = this.data;
    if (!d) return this.paintOverlay();
    const dark = matchMedia("(prefers-color-scheme: dark)").matches;
    ctx.font = `600 11px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`;
    ctx.textBaseline = "middle";
    for (const r of this.rects) {
      const isDir = d.dir[r.i] === 1;
      if (isDir) {
        if (r.chain) continue;
        const shade = dark ? 255 : 0;
        const a = dark ? 0.035 + Math.min(r.depth, 6) * 0.012 : 0.03 + Math.min(r.depth, 6) * 0.018;
        ctx.fillStyle = `rgba(${shade},${shade},${shade},${a})`;
        roundRect(ctx, r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1, Math.min(6, r.w / 4, r.h / 4));
        ctx.fill();
        if (r.label) {
          ctx.fillStyle = dark ? "rgba(255,255,255,0.78)" : "rgba(0,0,0,0.62)";
          ctx.fillText(ellipsizeStart(ctx, r.text ?? d.name[r.i], r.w - 14), r.x + 7, r.y + HEADER / 2 + 1);
        }
        continue;
      }
      const c = color(kindOf(d.ext[r.i]));
      const rad = r.w > 8 && r.h > 8 ? Math.min(4, r.w / 4, r.h / 4) : 0;
      const x = r.x + 0.5, y = r.y + 0.5, w = Math.max(0.6, r.w - 1), h = Math.max(0.6, r.h - 1);
      if (w < 6 || h < 6) {
        ctx.fillStyle = c;
        ctx.fillRect(x, y, w, h);
        continue;
      }
      // Cushion: light from the top-left, soft falloff to the far corner.
      const g = ctx.createLinearGradient(x, y, x + w * 0.7, y + h);
      g.addColorStop(0, mix(c, "#ffffff", dark ? 0.16 : 0.3));
      g.addColorStop(0.55, c);
      g.addColorStop(1, mix(c, "#000000", dark ? 0.32 : 0.2));
      ctx.fillStyle = g;
      roundRect(ctx, x, y, w, h, rad);
      ctx.fill();
      if (w > 22 && h > 22) {
        const rg = ctx.createRadialGradient(x + w * 0.32, y + h * 0.28, 0, x + w * 0.32, y + h * 0.28, Math.max(w, h) * 0.7);
        rg.addColorStop(0, "rgba(255,255,255,0.22)");
        rg.addColorStop(1, "rgba(255,255,255,0)");
        ctx.fillStyle = rg;
        ctx.fill();
      }
      if (w > 70 && h > 24) {
        ctx.fillStyle = "rgba(255,255,255,0.95)";
        ctx.font = `500 11px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`;
        ctx.fillText(ellipsize(ctx, d.name[r.i], w - 12), x + 6, y + 11);
        ctx.font = `600 11px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`;
      }
    }
    this.paintOverlay();
  }

  private paintOverlay() {
    const ctx = this.over.getContext("2d")!;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.clearRect(0, 0, this.w, this.h);
    const ring = (i: number, style: string, width: number) => {
      const r = this.rectOf(i);
      if (!r) return;
      ctx.strokeStyle = style;
      ctx.lineWidth = width;
      roundRect(ctx, r.x + width / 2, r.y + width / 2, r.w - width, r.h - width, Math.min(6, r.w / 4, r.h / 4));
      ctx.stroke();
    };
    if (this.hover >= 0 && this.hover !== this.selected) ring(this.hover, "rgba(255,255,255,0.85)", 1.5);
    if (this.selected >= 0) {
      const r = this.rectOf(this.selected);
      if (r) {
        ctx.fillStyle = "rgba(0,0,0,0.0)";
        ring(this.selected, "#ffffff", 3);
        ring(this.selected, getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#0a6cff", 1.5);
      }
    }
  }

  /** Deepest rect under the point. */
  private hit(x: number, y: number): number {
    let found = -1;
    for (const r of this.rects) {
      if (x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h) found = r.i;
    }
    return found;
  }
}

type Item = { k: number; a: number };

function squarify(items: Item[], x: number, y: number, w: number, h: number, out: (k: number, x: number, y: number, w: number, h: number) => void) {
  let rest = items.slice();
  while (rest.length) {
    const short = Math.min(w, h);
    let row: Item[] = [];
    let best = Infinity;
    let i = 0;
    for (; i < rest.length; i++) {
      const trial = row.concat(rest[i]);
      const s = worst(trial, short);
      if (s > best) break;
      best = s;
      row = trial;
    }
    rest = rest.slice(row.length);
    const sum = row.reduce((s, o) => s + o.a, 0);
    if (w >= h) {
      const cw = sum / h;
      let cy = y;
      for (const o of row) {
        const ch = o.a / cw;
        out(o.k, x, cy, cw, ch);
        cy += ch;
      }
      x += cw;
      w -= cw;
    } else {
      const ch = sum / w;
      let cx = x;
      for (const o of row) {
        const cw = o.a / ch;
        out(o.k, cx, y, cw, ch);
        cx += cw;
      }
      y += ch;
      h -= ch;
    }
    if (w < 0.5 || h < 0.5) break;
  }
}

function worst(row: Item[], side: number) {
  let s = 0, mx = 0, mn = Infinity;
  for (const o of row) {
    s += o.a;
    mx = Math.max(mx, o.a);
    mn = Math.min(mn, o.a);
  }
  const s2 = s * s, w2 = side * side;
  return Math.max((w2 * mx) / s2, s2 / (w2 * mn));
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  if (r <= 0.5) ctx.rect(x, y, w, h);
  else ctx.roundRect(x, y, w, h, r);
}

function ellipsize(ctx: CanvasRenderingContext2D, s: string, max: number) {
  if (ctx.measureText(s).width <= max) return s;
  let lo = 0, hi = s.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (ctx.measureText(s.slice(0, mid) + "…").width <= max) lo = mid;
    else hi = mid - 1;
  }
  return lo > 0 ? s.slice(0, lo) + "…" : "";
}

/** Keep the end of a chain label ("…› steamapps › common") visible. */
function ellipsizeStart(ctx: CanvasRenderingContext2D, s: string, max: number) {
  if (ctx.measureText(s).width <= max) return s;
  let lo = 0, hi = s.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (ctx.measureText("…" + s.slice(mid)).width <= max) hi = mid;
    else lo = mid + 1;
  }
  return lo < s.length ? "…" + s.slice(lo) : "";
}

const cache = new Map<string, string>();
function mix(a: string, b: string, t: number) {
  const key = a + b + t;
  const hit = cache.get(key);
  if (hit) return hit;
  const pa = parseInt(a.slice(1), 16), pb = parseInt(b.slice(1), 16);
  const ch = (s: number) => Math.round(((pa >> s) & 255) * (1 - t) + ((pb >> s) & 255) * t);
  const v = `rgb(${ch(16)},${ch(8)},${ch(0)})`;
  cache.set(key, v);
  return v;
}
