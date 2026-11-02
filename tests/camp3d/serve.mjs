// Test-only static server for the Camp 3D editor without the Tauri app.
//   node tests/camp3d/serve.mjs [port]
// Serves static/ at the root; /camp3d/index.html gets tauri_stub.js injected before shim.js (a fake
// window.__TAURI__.core.invoke backed by an in-memory store seeded from samples/camp3d/maps, served at /__test/seed).
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '../..');
const STATIC = path.join(ROOT, 'static');
const SAMPLE = path.join(ROOT, 'samples/camp3d/maps');
const PORT = +(process.argv[2] || 5179);
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.jpg': 'image/jpeg', '.png': 'image/png', '.webp': 'image/webp', '.css': 'text/css' };

http.createServer((req, res) => {
  const url = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (url === '/__test/seed') {
    const docs = {};
    for (const f of fs.readdirSync(SAMPLE)) if (f.endsWith('.json')) docs['maps/' + f.slice(0, -5)] = JSON.parse(fs.readFileSync(path.join(SAMPLE, f), 'utf8'));
    res.writeHead(200, { 'content-type': 'application/json' }); return res.end(JSON.stringify(docs));
  }
  if (url === '/__test/tauri_stub.js') { res.writeHead(200, { 'content-type': 'text/javascript' }); return res.end(fs.readFileSync(path.join(HERE, 'tauri_stub.js'))); }
  let file = path.join(STATIC, url.endsWith('/') ? url + 'index.html' : url);
  if (!file.startsWith(STATIC) || !fs.existsSync(file) || !fs.statSync(file).isFile()) { res.writeHead(404); return res.end('not found'); }
  let body = fs.readFileSync(file);
  if (url === '/camp3d/index.html') body = body.toString().replace('<script src="shim.js"></script>', '<script src="/__test/tauri_stub.js"></script>\n<script src="shim.js"></script>');
  res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
  res.end(body);
}).listen(PORT, '127.0.0.1', () => console.log(`camp3d test server on http://127.0.0.1:${PORT}/camp3d/index.html`));
