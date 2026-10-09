/* Drive demo/paph4x.html headlessly: the bench must boot from its own inlined
   engines, hash its own samples, reach a comparator-42 verdict, and fill every
   section — then load its inlined WebAssembly engine, switch to it, prove the
   two engines' reports are byte-identical, and run the attack sweep on both
   with identical rows.  Needs jsdom; skips cleanly without it. */
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
let JSDOM;
try { ({ JSDOM } = await import('jsdom')); }
catch (e) {
  console.log('bench harness skipped — jsdom is not installed (npm i -D jsdom to run it)');
  process.exit(0);
}
const FILE = join(dirname(fileURLToPath(import.meta.url)), '..', 'demo', 'paph4x.html');
if (!existsSync(FILE)) {
  console.error('demo/paph4x.html is missing — run `npm run build:bench` first');
  process.exit(2);
}
const html = readFileSync(FILE, 'utf8');
const dom = new JSDOM(html, { url: 'file:///bench/paph4x.html', runScripts: 'outside-only', pretendToBeVisual: true });
const { window } = dom;
/* canvas + ImageData stubs: jsdom has no 2D context */
window.ImageData = class { constructor(w, h) { this.width = w; this.height = h;
  this.data = new window.Uint8ClampedArray(w * h * 4); } };
const ctx = () => ({
  imageSmoothingEnabled: false, setTransform() {}, clearRect() {}, fillRect() {}, drawImage() {},
  putImageData() {}, beginPath() {}, moveTo() {}, lineTo() {}, stroke() {}, fill() {}, arc() {},
  save() {}, restore() {}, translate() {}, rotate() {}, fillText() {}, setLineDash() {},
  createLinearGradient: () => ({ addColorStop() {} }),
  createImageData: (w, h) => new window.ImageData(w, h),
  getImageData: (x, y, w, h) => new window.ImageData(w, h)
});
window.HTMLCanvasElement.prototype.getContext = ctx;
Object.defineProperty(window.HTMLCanvasElement.prototype, 'clientWidth', { get: () => 600 });
/* what a browser has and jsdom does not: the WebAssembly engine needs these */
window.WebAssembly = WebAssembly;
window.DecompressionStream = DecompressionStream;
window.Response = Response;
window.TextDecoder = TextDecoder;
for (const m of html.matchAll(/<script>([\s\S]*?)<\/script>/g)) window.eval(m[1]);

const API = window.paphjsx, $ = id => window.document.getElementById(id);
let pass = 0, fail = 0;
const ok = (n, c, x) => { (c ? pass++ : fail++); console.log('  ' + (c ? 'PASS' : 'FAIL') + ' ' + n + (x ? '  ' + x : '')); };
const sweep = () => new Promise((res, rej) => {
  API.sweep();
  const t0 = Date.now();
  const t = setInterval(() => {
    if (API.attackRows()) { clearInterval(t); res(API.attackRows()); }
    else if (Date.now() - t0 > 300000) { clearInterval(t); rej(new Error('sweep never finished')); }
  }, 50);
});

console.log('\nthe bench on its JavaScript engine (first paint, before WebAssembly loads)');
API.decide();   /* the page defers through rAF; the harness does not wait */
ok('the bench boots with both slots filled', !!API && !!API.slots().A && !!API.slots().B,
   API && API.slots().A ? API.slots().A.name + ' × ' + API.slots().B.name : '');
const r0 = API.report();
ok('it reaches a comparator-42 verdict', r0 && r0.comparator === 42, r0 ? r0.verdict + ' [' + r0.basis.join('+') + ']' : '');
ok('the profile is the shipped CAL-007-PROVISIONAL, container 3',
   API.profile().container === 3 && API.profile().comparator === 42 && $('profileChip').textContent === 'CAL-007-PROVISIO',
   $('profileChip').textContent);

console.log('\nthe bench on its WebAssembly engine');
const state = await API.ready();
ok('the inlined WebAssembly engine loads', state === 'ready', state + (API.engine().why ? ' — ' + API.engine().why : ''));
API.decide();
ok('the bench switched to it', API.engine().kind === 'wasm', $('engineChip').textContent);
const r = API.report();
ok('it reaches a comparator-42 verdict', r && r.comparator === 42, r ? r.verdict + ' [' + r.basis.join('+') + ']' : '');
ok('the same verdict as the JavaScript engine', r && r0 && r.verdict === r0.verdict && r.structural === r0.structural);
const chk = API.crossCheck();
ok('the two engines\' reports are byte-identical', !!chk && chk.same,
   chk ? chk.chars + ' characters · WebAssembly vs JavaScript' : 'no check ran');
ok('the cost section shows the cross-check', /byte-identical/.test($('engineStats').textContent));
ok('the screen ran and reported its pools', !!API.screen() && typeof API.screen().poolDirect === 'number',
   API.screen() ? (API.screen().pass ? 'pass' : 'unscreened') + ' ' + API.screen().poolDirect + '/' + API.screen().poolMirror : '');
ok('the verdict readout is painted', /Comparator 42/.test($('readout').textContent));
ok('the lattice lit exactly one cell',
   [...window.document.querySelectorAll('#latGrid .cell.on')].length === 1);
ok('all seven channels rendered', window.document.querySelectorAll('#chans .chan').length === 7);
ok('a channel shows raw and post-table values', /after the .* table/.test($('chans').textContent));
ok('the ten calibration curves are drawn', window.document.querySelectorAll('#curves svg.curve').length === 10);
ok('the artefact identity is shown', /320 bytes/.test($('profId').textContent));
ok('the wire ribbon has every section', $('ribbon').children.length === window.paphWire.SECTIONS.length);
ok('the hex view has bytes', $('hexview').textContent.length > 100);
const drawn = /an accepted model kept it \((\d+)\)/.exec($('corrLegend').textContent);
ok('the drawn inliers match the comparator\'s count',
   !!drawn && (+drawn[1] === r.totalInliers || (r.geoWeakInliers > 0 && +drawn[1] === 0)),
   drawn ? 'drawn ' + drawn[1] + ' · reported ' + r.totalInliers : 'legend missing');
ok('the replay never contradicts the report', !/replay disagrees/.test($('corrLegend').textContent));
ok('cost reports the comparator and the screen',
   /Compare/.test($('costStats').textContent) && /Screen/.test($('costStats').textContent) &&
   /budget of 25 ms/.test($('costStats').textContent));

const tw = Date.now();
const rowsW = (await sweep()).map(x => JSON.stringify(x));
const msW = Date.now() - tw;
ok('the attack sweep ran every transform plus the identity anchor',
   rowsW.length === Object.keys(API.transforms).length + 1, rowsW.length + ' rows in ' + msW + ' ms');
const rows = API.attackRows();
ok('the anchor row reads Identical', rows.some(x => x.v === 'Identical'));
ok('every row carries a verdict and a screen result',
   rows.every(x => x.v && (x.screen === 'pass' || x.screen === 'unscreened')),
   rows.slice(0, 3).map(x => x.label + ': ' + x.v + ' / ' + x.screen).join(' | '));
ok('the mirror survives as a copy', rows.some(x => /mirror/i.test(x.label) && (x.v === 'Copy' || x.v === 'Identical')));
ok('every certified row carries a diversity reading',
   rows.filter(x => x.v === 'Copy' || x.v === 'Identical').every(x => typeof x.d === 'number'));

console.log('\nthe same sweep on the JavaScript engine');
API.setEngine('js');
API.decide();
ok('the bench switched back to JavaScript', API.engine().kind === 'js', $('engineChip').textContent);
const tj = Date.now();
const rowsJ = (await sweep()).map(x => JSON.stringify(x));
const msJ = Date.now() - tj;
ok('every sweep row is identical on both engines', rowsJ.length === rowsW.length && rowsJ.every((x, i) => x === rowsW[i]),
   `JavaScript ${msJ} ms, WebAssembly ${msW} ms`);
console.log('\n' + pass + ' passed, ' + fail + ' failed');
process.exit(fail ? 1 : 0);
