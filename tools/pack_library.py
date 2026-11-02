"""Pack the Camp 3D model library into split zip parts + library-manifest.json for a GitHub release.

Usage: python tools/pack_library.py <library dir, e.g. E:/AI/bg3d> <out dir> <version> <release base url>
  e.g. ... E:/AI/bg3d E:/Dev/MapSmithEE-scratch/library v1 https://github.com/sad0/MapSmithEE/releases/download/library-v1
Only the folders the render reads are packed (models.json, assets, mesh, proxies, tex, ext, CREDITS.md). Each part stays
under 1.9 GB (GitHub's limit per release file is 2 GB). Images are stored without recompression.
"""
import hashlib, json, os, sys, zipfile

SRC, OUT, VER, BASE = sys.argv[1:5]
KEEP = ['models.json', 'CREDITS.md', 'assets', 'mesh', 'proxies', 'tex', 'ext']
LIMIT = 1_900_000_000
STORED = ('.jpg', '.jpeg', '.png', '.webp', '.glb', '.zip')

files = []
for k in KEEP:
    p = os.path.join(SRC, k)
    if os.path.isfile(p):
        files.append(k)
    else:
        for root, _, names in os.walk(p):
            for n in sorted(names):
                files.append(os.path.relpath(os.path.join(root, n), SRC).replace('\\', '/'))
os.makedirs(OUT, exist_ok=True)
parts, cur, size, n = [], None, 0, 0


def close():
    global cur
    if cur:
        cur.close()
        cur = None


for f in files:
    fs = os.path.getsize(os.path.join(SRC, f))
    if cur is None or size + fs > LIMIT:
        close()
        n += 1
        name = f'mapsmithee-library-{VER}.part{n}.zip'
        cur = zipfile.ZipFile(os.path.join(OUT, name), 'w', zipfile.ZIP_DEFLATED, compresslevel=6)
        parts.append(name)
        size = 0
    cur.write(os.path.join(SRC, f), f, compress_type=zipfile.ZIP_STORED if f.lower().endswith(STORED) else zipfile.ZIP_DEFLATED)
    size += fs
close()

manifest = {'version': VER, 'parts': []}
for name in parts:
    p = os.path.join(OUT, name)
    h = hashlib.sha256()
    with open(p, 'rb') as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b''):
            h.update(chunk)
    manifest['parts'].append({'name': name, 'url': f'{BASE}/{name}', 'size': os.path.getsize(p), 'sha256': h.hexdigest()})
json.dump(manifest, open(os.path.join(OUT, 'library-manifest.json'), 'w'), indent=1)
print(len(files), 'files in', len(parts), 'parts:', [(x['name'], x['size'] // 1_000_000) for x in manifest['parts']])
