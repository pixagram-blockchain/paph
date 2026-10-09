/**
 * The real corpus: the artworks Hivemind lists on the Pixa chain, decoded to RGBA.
 *
 *     node tools/chain-corpus.mjs [--out DIR] [--rpc URL] [--max N] [--delay MS]
 *
 * Pages through `condenser_api.get_discussions_by_created` (Hivemind, 20 posts a page, newest
 * first) on https://api.pixagram.com, keeps the artworks — `json_metadata.format` "image", or a
 * body that is a data URI and nothing else — and skips deleted ones (Pixagram deletes by editing
 * the body to "deleted").  An artwork's body is the image itself, `data:image/webp;base64,…`
 * (WebP, lossless as the app writes it; PNG possible), so nothing but the chain is read.
 *
 * Decoding uses the codecs pixagram-search decodes with (`@jsquash/webp`, `@jsquash/png`: libwebp
 * and the squoosh PNG decoder compiled to WebAssembly), so the pixels — and the wires hashed from
 * them — are the ones the search engine sees.  They are devDependencies of this repository
 * (`npm install`); nothing in the package needs them.
 *
 * Writes, under DIR (default rust/target/chain-corpus):
 *   rgba/<author>__<permlink>.rgba   [u32 LE width][u32 LE height][RGBA], the xbench --dump format
 *   manifest.jsonl                   one artwork a line, oldest first: author, permlink, created,
 *                                    title, tags, nsfw, mime, lossy, bytes, sha256 of the image
 *                                    bytes, width, height, file
 *   index.tsv                        the same, tab-separated, for rust/src/bin/sibench.rs
 *   snapshot.json                    head block and time at the start of the walk, counts
 *
 * A snapshot: the chain moves on, and re-running it later gives more works.  `sibench corpus
 * --chain DIR` builds the PAPH-SI corpus from it (docs/SPEC-SI-paph-si.md §9.7).
 */
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const arg = (k, d) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : d; };
const OUT = arg('--out', join(root, 'rust', 'target', 'chain-corpus'));
const RPC = arg('--rpc', 'https://api.pixagram.com');
const MAX = +arg('--max', Infinity);
const DELAY = +arg('--delay', 100);
/** the search engine's cap: 4 bytes a pixel inside a 128 MB isolate */
const MAX_PIXELS = 2048 * 2048;

// ---- codecs -------------------------------------------------------------------------

const require = createRequire(import.meta.url);
let webpDecode, pngDecode;
try {
  const webp = await import('@jsquash/webp/decode.js');
  const png = await import('@jsquash/png/decode.js');
  await webp.init(await WebAssembly.compile(readFileSync(require.resolve('@jsquash/webp/codec/dec/webp_dec.wasm'))));
  await png.init(await WebAssembly.compile(readFileSync(require.resolve('@jsquash/png/codec/pkg/squoosh_png_bg.wasm'))));
  webpDecode = webp.default;
  pngDecode = png.decode;
} catch (e) {
  console.error('chain-corpus: the image codecs are missing — run `npm install` (devDependencies @jsquash/webp, @jsquash/png)\n', e.message);
  process.exit(2);
}

const ascii = (b, o, n) => String.fromCharCode(...b.subarray(o, o + n));
/** The container, as pixagram-search's sniff() reads it. */
function sniff(b) {
  if (b.length >= 12 && ascii(b, 0, 4) === 'RIFF' && ascii(b, 8, 4) === 'WEBP') {
    let lossy = false;
    for (let off = 12; off + 8 <= b.length;) {
      const cc = ascii(b, off, 4);
      const size = b[off + 4] | (b[off + 5] << 8) | (b[off + 6] << 16) | (b[off + 7] << 24);
      if (cc === 'VP8 ') { lossy = true; break; }
      if (cc === 'VP8L') break;
      off += 8 + size + (size & 1);
    }
    return { format: 'webp', lossy };
  }
  if (b.length >= 8 && b[0] === 0x89 && ascii(b, 1, 3) === 'PNG') return { format: 'png', lossy: false };
  return { format: 'unknown', lossy: false };
}

async function decode(bytes) {
  const c = sniff(bytes);
  const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  let img;
  if (c.format === 'webp') img = await webpDecode(buf);
  else if (c.format === 'png') img = await pngDecode(buf);
  else throw new Error('unsupported container');
  if (img.width * img.height > MAX_PIXELS) throw new Error(`too large: ${img.width}x${img.height}`);
  return { ...c, width: img.width, height: img.height, data: new Uint8Array(img.data.buffer, img.data.byteOffset, img.data.byteLength) };
}

// ---- chain --------------------------------------------------------------------------

let id = 1;
async function rpc(method, params, tries = 4) {
  for (let k = 0; ; k++) {
    try {
      const r = await fetch(RPC, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: id++, method, params }) });
      if (r.status >= 500 || r.status === 429) throw new Error(`HTTP ${r.status}`);
      const j = await r.json();
      if (j.error) throw Object.assign(new Error(j.error.message), { rpc: true });
      return j.result;
    } catch (e) {
      if (e.rpc || k + 1 >= tries) throw e;
      await new Promise(res => setTimeout(res, 500 * (k + 1)));
    }
  }
}

const DATA_URI_WHOLE = /^\s*data:image\/([a-z0-9.+-]+);base64,([A-Za-z0-9+/=\s]+?)\s*$/i;
function jsonMeta(jm) {
  if (jm && typeof jm === 'object') return jm;
  try { const v = JSON.parse(jm); return v && typeof v === 'object' ? v : {}; } catch { return {}; }
}

// ---- the walk -----------------------------------------------------------------------

rmSync(join(OUT, 'rgba'), { recursive: true, force: true });
mkdirSync(join(OUT, 'rgba'), { recursive: true });
const dgp = await rpc('condenser_api.get_dynamic_global_properties', []);
console.log(`chain-corpus: ${RPC}, head block ${dgp.head_block_number} (${dgp.time} UTC) → ${OUT}`);

const works = [];
const counts = { posts: 0, artworks: 0, deleted: 0, blog: 0, undecodable: 0, tooLarge: 0, other: 0 };
let start = null;
const seen = new Set();
const t0 = Date.now();
for (let page = 0; works.length < MAX; page++) {
  const list = await rpc('condenser_api.get_discussions_by_created', [{ tag: '', limit: 20, ...(start ? { start_author: start.author, start_permlink: start.permlink } : {}) }]);
  const fresh = list.filter(p => !seen.has(`${p.author}/${p.permlink}`));
  if (!fresh.length) break;
  for (const p of fresh) {
    seen.add(`${p.author}/${p.permlink}`);
    counts.posts++;
    const body = typeof p.body === 'string' ? p.body : '';
    if (body.trim().toLowerCase() === 'deleted') { counts.deleted++; continue; }
    const jm = jsonMeta(p.json_metadata);
    const m = DATA_URI_WHOLE.exec(body);
    const fmt = typeof jm.format === 'string' ? jm.format.toLowerCase() : '';
    if (!m) { if (fmt === 'image' || fmt === 'artwork') counts.other++; else counts.blog++; continue; }
    const bytes = new Uint8Array(Buffer.from(m[2].replace(/\s+/g, ''), 'base64'));
    let img;
    try {
      img = await decode(bytes);
    } catch (e) {
      if (/too large/.test(e.message)) counts.tooLarge++; else counts.undecodable++;
      continue;
    }
    const file = `rgba/${p.author}__${p.permlink}.rgba`;
    const head = new Uint8Array(8);
    new DataView(head.buffer).setUint32(0, img.width, true);
    new DataView(head.buffer).setUint32(4, img.height, true);
    writeFileSync(join(OUT, file), Buffer.concat([head, img.data]));
    works.push({
      author: p.author, permlink: p.permlink, created: p.created, title: (p.title || '').slice(0, 200),
      tags: Array.isArray(jm.tags) ? jm.tags.slice(0, 16) : [], nsfw: jm.nsfw === true || jm.nsfw === 'true',
      mime: `image/${m[1].toLowerCase()}`, lossy: img.lossy, bytes: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'), width: img.width, height: img.height, file,
    });
    counts.artworks++;
    if (works.length >= MAX) break;
  }
  start = list[list.length - 1];
  if (page % 25 === 0) console.log(`  page ${page}: ${counts.posts} posts, ${works.length} artworks, oldest ${start.created} (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
  if (DELAY) await new Promise(res => setTimeout(res, DELAY));
}

works.sort((a, b) => (a.created < b.created ? -1 : a.created > b.created ? 1 : `${a.author}/${a.permlink}` < `${b.author}/${b.permlink}` ? -1 : 1));
writeFileSync(join(OUT, 'manifest.jsonl'), works.map(w => JSON.stringify(w)).join('\n') + '\n');
writeFileSync(join(OUT, 'index.tsv'), works.map(w => [w.author, w.permlink, w.created, w.width, w.height, w.sha256, w.lossy ? 1 : 0, w.nsfw ? 1 : 0, w.file].join('\t')).join('\n') + '\n');
const manifestSha = createHash('sha256').update(readFileSync(join(OUT, 'manifest.jsonl'))).digest('hex');
const snapshot = { rpc: RPC, head_block: dgp.head_block_number, time: dgp.time, ...counts, manifest_sha256: manifestSha };
writeFileSync(join(OUT, 'snapshot.json'), JSON.stringify(snapshot, null, 2) + '\n');
console.log(`chain-corpus: ${counts.posts} posts walked — ${counts.artworks} artworks decoded, ${counts.blog} blog posts, ${counts.deleted} deleted, ${counts.other} artwork posts without a whole-body image, ${counts.undecodable} undecodable, ${counts.tooLarge} over ${MAX_PIXELS} px; manifest ${manifestSha.slice(0, 16)} (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
