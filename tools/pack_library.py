"""Pack the Camp 3D model library into split zip parts + library-manifest.json for a GitHub release.

Usage: python tools/pack_library.py <library dir, e.g. E:/AI/bg3d> <out dir> <version> <release base url>
  e.g. ... E:/AI/bg3d E:/Dev/MapSmithEE-scratch/library v1 https://github.com/sad0/MapSmithEE/releases/download/library-v1
Only the folders the render reads are packed (models.json, assets, mesh, proxies, tex, ext, CREDITS.md). models.json
points at absolute paths (the library itself, and E:/AI/objaverse/glb): those files are packed too, under glb/ when they
live outside the library, and the packed models.json uses paths relative to the library (the Blender scripts resolve
them from there). Each part stays under 1.9 GB (GitHub's limit per release file is 2 GB). Images and GLBs are stored
without recompression.
"""
import hashlib, json, os, sys, zipfile

SRC, OUT, VER, BASE = sys.argv[1:5]
FOLDERS = ['assets', 'mesh', 'proxies', 'tex', 'ext']
LIMIT = 1_900_000_000
STORED = ('.jpg', '.jpeg', '.png', '.webp', '.glb', '.zip')

files = []   # (source path, name in the archive)
for k in FOLDERS:
    for root, _, names in os.walk(os.path.join(SRC, k)):
        for n in sorted(names):
            full = os.path.join(root, n)
            files.append((full, os.path.relpath(full, SRC).replace(os.sep, '/')))
if os.path.isfile(os.path.join(SRC, 'CREDITS.md')):
    files.append((os.path.join(SRC, 'CREDITS.md'), 'CREDITS.md'))

root_src = os.path.abspath(SRC).replace(os.sep, '/').rstrip('/') + '/'
extra = {}


def relink(v):
    if isinstance(v, dict):
        return {k: relink(x) for k, x in v.items()}
    if isinstance(v, list):
        return [relink(x) for x in v]
    if isinstance(v, str) and len(v) > 2 and v[1] == ':' and os.path.isfile(v):
        p = os.path.abspath(v).replace(os.sep, '/')
        if p.lower().startswith(root_src.lower()):
            return p[len(root_src):]
        rel = 'glb/' + os.path.basename(p)
        extra.setdefault(rel, p)
        return rel
    return v


os.makedirs(OUT, exist_ok=True)
models = relink(json.load(open(os.path.join(SRC, 'models.json'), encoding='utf-8')))
rel_models = os.path.join(OUT, 'models.json')
json.dump(models, open(rel_models, 'w', encoding='utf-8'), ensure_ascii=False)
files.append((rel_models, 'models.json'))
files += [(p, rel) for rel, p in sorted(extra.items())]

parts, cur, size, n = [], None, 0, 0
for src, name_in_zip in files:
    fs = os.path.getsize(src)
    if cur is None or size + fs > LIMIT:
        if cur:
            cur.close()
        n += 1
        part = f'mapsmithee-library-{VER}.part{n}.zip'
        cur = zipfile.ZipFile(os.path.join(OUT, part), 'w', zipfile.ZIP_DEFLATED, compresslevel=6)
        parts.append(part)
        size = 0
    stored = name_in_zip.lower().endswith(STORED)
    cur.write(src, name_in_zip, compress_type=zipfile.ZIP_STORED if stored else zipfile.ZIP_DEFLATED)
    size += fs
if cur:
    cur.close()

manifest = {'version': VER, 'parts': []}
for part in parts:
    p = os.path.join(OUT, part)
    h = hashlib.sha256()
    with open(p, 'rb') as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b''):
            h.update(chunk)
    manifest['parts'].append({'name': part, 'url': f'{BASE}/{part}', 'size': os.path.getsize(p), 'sha256': h.hexdigest()})
json.dump(manifest, open(os.path.join(OUT, 'library-manifest.json'), 'w'), indent=1)
print(len(files), 'files,', len(extra), 'from outside the library, in', len(parts), 'parts:',
      [(x['name'], x['size'] // 1_000_000) for x in manifest['parts']])
