/**
 * The native timing corpus (rust/bench.sh): the evidence bench's own pixel-art
 * samples and their transforms, plus the parity `work` image.  Raw RGBA dumps,
 * [u32 w][u32 h][rgba], deterministic.
 *
 *     node tools/gen-corpus.mjs [outdir]        (default rust/target/bench-corpus)
 *
 * The samples are taken from demo/bench4x/bench.js itself — the same generator
 * code the bench runs — so the timings measure what the bench shows.
 */
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = process.argv[2] || join(root, 'rust', 'target', 'bench-corpus');
mkdirSync(OUT, { recursive: true });

class ImageData { constructor(w, h) { this.width = w; this.height = h; this.data = new Uint8ClampedArray(w * h * 4); } }
const src = readFileSync(join(root, 'demo', 'bench4x', 'bench.js'), 'utf8');
const start = src.indexOf('function lcg(');
const end = src.indexOf('/* ================================================================ *\n * plates + hashing');
if (start < 0 || end < 0) throw new Error('gen-corpus: bench.js no longer has the sample generators where expected');
const ctx = { ImageData, Math, Map, Array, Object, String };
vm.createContext(ctx);
vm.runInContext(src.slice(start, end) + '\nthis.SAMPLES = SAMPLES; this.XF = XF;', ctx);
const { SAMPLES, XF } = ctx;

function work(w, h, seed, alpha) {
  const px = new Uint8Array(w * h * 4);
  let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) << 2, cx = x - w / 2, cy = y - h / 2;
    if (alpha && (cx * cx) / (w * w / 5) + (cy * cy) / (h * h / 4.5) >= 1) { px[o + 3] = 0; continue; }
    px[o] = (x * 7 + y * 3 + seed) % 251; px[o + 1] = (y * 11 + x * 5) % 253;
    px[o + 2] = ((x ^ y) * 13) % 247; px[o + 3] = 255;
  }
  for (let i = 0; i < 40; i++) {
    const bx = (R() * (w - 10)) | 0, by = (R() * (h - 10)) | 0;
    const r = (R() * 255) | 0, g = (R() * 255) | 0, b = (R() * 255) | 0;
    for (let dy = 0; dy < 6; dy++) for (let dx = 0; dx < 6; dx++) {
      const o = ((by + dy) * w + bx + dx) << 2;
      if (px[o + 3] === 0) continue;
      px[o] = r; px[o + 1] = g; px[o + 2] = b;
    }
  }
  return { data: px, width: w, height: h };
}

function dump(name, im) {
  const head = Buffer.alloc(8);
  head.writeUInt32LE(im.width, 0);
  head.writeUInt32LE(im.height, 4);
  writeFileSync(join(OUT, `${name}.rgba`), Buffer.concat([head, Buffer.from(im.data.buffer, im.data.byteOffset, im.data.byteLength)]));
  console.log(name.padEnd(22), im.width + 'x' + im.height);
}

dump('work-512x384', work(512, 384, 3, false));
dump('work-alpha-301x97', work(301, 97, 7, true));
const sprite = SAMPLES.sprite(), scene = SAMPLES.scene(), banner = SAMPLES.banner(), tile = SAMPLES.tile();
dump('sprite', sprite); dump('scene', scene); dump('banner', banner); dump('tile', tile);
dump('scene-1024x768', SAMPLES.scene(1024, 768));
for (const k of Object.keys(XF)) dump('sprite-' + k, XF[k].fn(sprite));
dump('scene-mirror', XF.mirror.fn(scene));
dump('scene-crop', XF.crop.fn(scene));
dump('scene-rescale', XF.rescale.fn(scene));
