/**
 * Deterministic test images, integer arithmetic only, shared by the parity
 * and benchmark scripts.  Each is { px: Uint8Array RGBA, w, h }.
 */
export function work(w, h, seed) {
  const px = new Uint8Array(w * h * 4); let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) << 2;
    px[o] = (x * 7 + y * 3 + seed) % 251; px[o + 1] = (y * 11 + x * 5) % 253;
    px[o + 2] = ((x ^ y) * 13) % 247; px[o + 3] = 255;
  }
  for (let i = 0; i < 40; i++) {
    const bx = (R() * (w - 10)) | 0, by = (R() * (h - 10)) | 0;
    const r = (R() * 255) | 0, g = (R() * 255) | 0, b = (R() * 255) | 0;
    for (let dy = 0; dy < 6; dy++) for (let dx = 0; dx < 6; dx++) {
      const o = ((by + dy) * w + bx + dx) << 2; px[o] = r; px[o + 1] = g; px[o + 2] = b;
    }
  }
  return { px, w, h };
}

/** Ordered-dither pixel art: a sky, a ground, buildings with windows. */
export function scene(w, h, seed) {
  let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  const pal = [];
  for (let i = 0; i < 8; i++) pal.push([28 + (R() * 205) | 0, 22 + (R() * 210) | 0, 44 + (R() * 196) | 0]);
  const B = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
  const px = new Uint8Array(w * h * 4);
  const put = (x, y, c) => {
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    const o = (y * w + x) << 2; px[o] = c[0]; px[o + 1] = c[1]; px[o + 2] = c[2]; px[o + 3] = 255;
  };
  const shade = (x, y, a, b, t) => put(x, y, B[((y & 3) << 2) | (x & 3)] < t ? a : b);
  const hz = (h * 0.58) | 0;
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    if (y < hz) shade(x, y, pal[0], pal[1], Math.max(0, Math.min(15, 15 - ((y * 15 / hz) | 0))));
    else shade(x, y, pal[2], pal[3], Math.max(0, Math.min(15, ((y - hz) * 15 / (h - hz)) | 0)));
  }
  const nb = Math.max(9, Math.round(w * h / 2600));
  for (let i = 0; i < nb; i++) {
    const bw = 12 + ((R() * 26) | 0), bh = 16 + ((R() * 44) | 0);
    const bx = (R() * (w - bw - 2)) | 0, by = hz - bh + ((R() * 14) | 0);
    const a = pal[4 + ((R() * 4) | 0)], b = pal[(R() * 4) | 0];
    for (let y = 0; y < bh; y++) for (let x = 0; x < bw; x++) {
      if (x === 0 || y === 0 || x === bw - 1 || y === bh - 1) put(bx + x, by + y, [12, 14, 22]);
      else if (x % 5 === 2 && y % 6 === 3) put(bx + x, by + y, [250, 224, 150]);
      else shade(bx + x, by + y, a, b, 6 + (x % 3) * 3);
    }
  }
  return { px, w, h };
}

/** A transparent-background sprite: the silhouette channel has something to read. */
export function sprite(w, h, seed) {
  const px = new Uint8Array(w * h * 4); let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) << 2;
    const dx = (x - w / 2) / (w * 0.35), dy = (y - h / 2) / (h * 0.42);
    if (dx * dx + dy * dy > 1) continue;
    const t = ((x >> 2) + (y >> 2) + seed) & 3;
    px[o] = [30, 90, 160, 220][t]; px[o + 1] = [200, 60, 120, 40][(t + 1) & 3];
    px[o + 2] = (x * 9 + y * 5 + seed) & 255; px[o + 3] = 255;
  }
  for (let i = 0; i < 12; i++) {
    const bx = (w * 0.3 + R() * w * 0.3) | 0, by = (h * 0.3 + R() * h * 0.3) | 0;
    const c = [(R() * 255) | 0, (R() * 255) | 0, (R() * 255) | 0];
    for (let dy = 0; dy < 4; dy++) for (let dx = 0; dx < 4; dx++) {
      const o = ((by + dy) * w + bx + dx) << 2;
      if (px[o + 3]) { px[o] = c[0]; px[o + 1] = c[1]; px[o + 2] = c[2]; }
    }
  }
  return { px, w, h };
}

export function flat(w, h, c) {
  const px = new Uint8Array(w * h * 4);
  for (let i = 0; i < px.length; i += 4) { px[i] = c[0]; px[i + 1] = c[1]; px[i + 2] = c[2]; px[i + 3] = 255; }
  return { px, w, h };
}

function copyPx(src, s, dst, d) {
  dst[d] = src[s]; dst[d + 1] = src[s + 1]; dst[d + 2] = src[s + 2]; dst[d + 3] = src[s + 3];
}

export function mirror({ px, w, h }) {
  const o = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) copyPx(px, (y * w + (w - 1 - x)) << 2, o, (y * w + x) << 2);
  return { px: o, w, h };
}

export function rot90({ px, w, h }) {
  const o = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) copyPx(px, (y * w + x) << 2, o, (x * h + (h - 1 - y)) << 2);
  return { px: o, w: h, h: w };
}

export function crop({ px, w, h }, f = 0.7, ax = 0.5, ay = 0.5) {
  const nw = Math.max(1, (w * f) | 0), nh = Math.max(1, (h * f) | 0);
  const x0 = ((w - nw) * ax) | 0, y0 = ((h - nh) * ay) | 0, o = new Uint8Array(nw * nh * 4);
  for (let y = 0; y < nh; y++) for (let x = 0; x < nw; x++) copyPx(px, ((y0 + y) * w + x0 + x) << 2, o, (y * nw + x) << 2);
  return { px: o, w: nw, h: nh };
}

export function upscale({ px, w, h }, k = 2) {
  const nw = w * k, nh = h * k, o = new Uint8Array(nw * nh * 4);
  for (let y = 0; y < nh; y++) for (let x = 0; x < nw; x++) copyPx(px, (((y / k) | 0) * w + ((x / k) | 0)) << 2, o, (y * nw + x) << 2);
  return { px: o, w: nw, h: nh };
}

/** Box-filtered resample to (nw, nh): what a re-export at a new size does. */
export function resample({ px, w, h }, nw, nh) {
  const o = new Uint8Array(nw * nh * 4);
  for (let y = 0; y < nh; y++) for (let x = 0; x < nw; x++) {
    const x0 = (x * w / nw) | 0, x1 = Math.max(x0 + 1, ((x + 1) * w / nw) | 0);
    const y0 = (y * h / nh) | 0, y1 = Math.max(y0 + 1, ((y + 1) * h / nh) | 0);
    const acc = [0, 0, 0, 0]; let n = 0;
    for (let yy = y0; yy < y1; yy++) for (let xx = x0; xx < x1; xx++) {
      const s = (yy * w + xx) << 2; for (let c = 0; c < 4; c++) acc[c] += px[s + c]; n++;
    }
    for (let c = 0; c < 4; c++) o[((y * nw + x) << 2) + c] = (acc[c] / n) | 0;
  }
  return { px: o, w: nw, h: nh };
}

export function invert({ px, w, h }) {
  const o = px.slice();
  for (let i = 0; i < o.length; i += 4) { o[i] = 255 - o[i]; o[i + 1] = 255 - o[i + 1]; o[i + 2] = 255 - o[i + 2]; }
  return { px: o, w, h };
}

export function recolour({ px, w, h }) {
  const o = px.slice();
  for (let i = 0; i < o.length; i += 4) { const r = o[i]; o[i] = o[i + 1]; o[i + 1] = o[i + 2]; o[i + 2] = r; }
  return { px: o, w, h };
}

/** `guest` pasted into `host` at (ox, oy); transparent guest pixels keep the host. */
export function paste(guest, host, ox, oy) {
  const o = host.px.slice();
  for (let y = 0; y < guest.h; y++) for (let x = 0; x < guest.w; x++) {
    const X = ox + x, Y = oy + y;
    if (X < 0 || Y < 0 || X >= host.w || Y >= host.h) continue;
    const s = (y * guest.w + x) << 2;
    if (guest.px[s + 3] === 0) continue;
    copyPx(guest.px, s, o, (Y * host.w + X) << 2);
  }
  return { px: o, w: host.w, h: host.h };
}

/** A named corpus: originals, their transforms, and unrelated works. */
export function corpus() {
  const W = work(128, 128, 42), S = scene(288, 200, 9137), P = sprite(96, 96, 5);
  return [
    ['work42', W], ['work1337', work(128, 128, 1337)], ['work42~mirror', mirror(W)],
    ['work42~crop', crop(W)], ['work42~2x', upscale(W, 2)], ['work42~invert', invert(W)],
    ['work42~recolour', recolour(W)], ['work42~rot90', rot90(W)],
    ['scene', S], ['scene~mirror', mirror(S)], ['scene~crop', crop(S, 0.6, 0.2, 0.8)],
    ['scene~3x', upscale(S, 3)], ['scene~resample', resample(S, 200, 139)],
    ['sprite', P], ['sprite~mirror', mirror(P)], ['sprite~paste', paste(P, scene(320, 220, 77), 140, 60)],
    ['other-scene', scene(256, 256, 4242)], ['other-sprite', sprite(96, 96, 77)],
    ['flat', flat(64, 48, [40, 90, 200])], ['tiny', work(9, 7, 3)]
  ];
}
