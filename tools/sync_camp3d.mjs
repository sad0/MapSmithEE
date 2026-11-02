#!/usr/bin/env node
// Import the Camp Map 3D editor (first written as a claude.ai artifact by the "LoRA - Camp" session) into
// static/camp3d/. MapSmithEE now OWNS the editor: static/camp3d/index.html is the reference copy, edited in place.
//
//   node tools/sync_camp3d.mjs [sourceDir] [--force]
//
// Always refreshes the assets: models.json, models/, thumbs.jpg, tex/ and vendor/ (three.js from tools/vendor/,
// three@0.147.0, MIT). Only changed files are copied (size or mtime). proxies/ and thumbs/ are not used by
// index.html and are not copied.
// index.html is a one-time import: an existing static/camp3d/index.html is never overwritten unless --force is given
// (that throws away MapSmithEE's own edits). The imported copy is patched:
//   - the three.js CDN scripts point at vendor/;
//   - <script src="shim.js"></script> goes in before the editor's main script (window.claude backed by Tauri).
//
// static/camp3d/models/ and static/camp3d/proxies/ are gitignored (already in .gitignore): ~119 MB of glTF JSON.
// static/camp3d/shim.js is hand-written and committed; never overwritten here.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..');
const DEFAULT_SRC = 'C:/Users/jnthn/AppData/Local/Temp/claude/E--SteamLibrary-steamapps-common-Baldur-s-Gate-Enhanced-Edition/401f8bea-5fa0-4451-b213-90608a8466a0/scratchpad/map3d';
const ARGS = process.argv.slice(2), FORCE = ARGS.includes('--force');
const SRC = path.resolve(ARGS.find(a => !a.startsWith('--')) || DEFAULT_SRC);
const DST = path.join(ROOT, 'static', 'camp3d');
const VENDOR_SRC = path.join(HERE, 'vendor');
const CDN = 'https://cdn.jsdelivr.net/npm/three@0.147.0/';
const VENDOR = ['build/three.min.js', 'examples/js/loaders/GLTFLoader.js', 'examples/js/controls/OrbitControls.js', 'examples/js/controls/TransformControls.js'];

let copied = 0, skipped = 0;
function copyFile(a, b) {
  const sa = fs.statSync(a);
  try {
    const sb = fs.statSync(b);
    if (sb.size === sa.size && Math.abs(sb.mtimeMs - sa.mtimeMs) < 2) { skipped++; return; }
  } catch {}
  fs.mkdirSync(path.dirname(b), { recursive: true });
  fs.copyFileSync(a, b); fs.utimesSync(b, sa.atime, sa.mtime); copied++;
}
function copyDir(a, b) {
  for (const e of fs.readdirSync(a, { withFileTypes: true })) {
    const pa = path.join(a, e.name), pb = path.join(b, e.name);
    if (e.isDirectory()) copyDir(pa, pb); else if (e.isFile()) copyFile(pa, pb);
  }
}

if (!fs.existsSync(path.join(SRC, 'index.html'))) { console.error(`No index.html in ${SRC}`); process.exit(1); }
fs.mkdirSync(DST, { recursive: true });

let html = fs.readFileSync(path.join(SRC, 'index.html'), 'utf8');
for (const f of ['models.json', 'thumbs.jpg']) copyFile(path.join(SRC, f), path.join(DST, f));
for (const d of ['models', 'tex']) copyDir(path.join(SRC, d), path.join(DST, d));
const importHtml = FORCE || !fs.existsSync(path.join(DST, 'index.html'));
const usedHtml = importHtml ? html : fs.readFileSync(path.join(DST, 'index.html'), 'utf8');
if (/["'`]\.?\/?proxies\//.test(usedHtml)) { console.log('index.html uses proxies/: copying it'); copyDir(path.join(SRC, 'proxies'), path.join(DST, 'proxies')); }

// (a) CDN three.js -> vendor/
for (const v of VENDOR) {
  const name = path.basename(v), from = path.join(VENDOR_SRC, name);
  if (!fs.existsSync(from)) { console.error(`Missing ${from}: download ${CDN}${v} into tools/vendor/`); process.exit(1); }
  copyFile(from, path.join(DST, 'vendor', name));
  if (!importHtml) continue;
  const tag = `<script src="${CDN}${v}"></script>`;
  if (!html.includes(tag)) { console.error(`Script tag not found in index.html: ${tag}`); process.exit(1); }
  html = html.replace(tag, `<script src="vendor/${name}"></script>`);
}
if (importHtml && html.includes('cdn.jsdelivr.net')) console.warn('Warning: index.html still references cdn.jsdelivr.net');

// (b) shim before the editor's main (inline) script
const SHIM = '<script src="shim.js"></script>';
if (importHtml && !html.includes(SHIM)) {
  const m = /<script>/.exec(html);
  if (!m) { console.error('Main <script> not found in index.html'); process.exit(1); }
  html = html.slice(0, m.index) + SHIM + '\n' + html.slice(m.index);
}
if (importHtml) { fs.writeFileSync(path.join(DST, 'index.html'), html); console.log('index.html imported'); }
else console.log('static/camp3d/index.html exists: kept (MapSmithEE owns it; --force to re-import from the source)');
if (!fs.existsSync(path.join(DST, 'shim.js'))) console.warn('Warning: static/camp3d/shim.js is missing (it is committed, not generated)');
console.log(`camp3d synced from ${SRC}: ${copied} files copied, ${skipped} unchanged`);
