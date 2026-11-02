"""Write CREDITS.md from the Camp 3D library shipped in static/camp3d (models.json + tex/).

Every model with a `credit` is listed with its author and licence. Poly Haven assets (models `src.asset`, ground
textures) get their authors from the Poly Haven API, cached in tools/polyhaven_authors.json.
Usage: python tools/gen_credits.py [static/camp3d] [E:/AI/bg3d/models.json]
(the editor's models.json lists the shipped models; the bg3d one holds their sources and credits)
"""
import json, os, sys, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIB = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, 'static', 'camp3d')
META = sys.argv[2] if len(sys.argv) > 2 else 'E:/AI/bg3d/models.json'
CACHE = os.path.join(ROOT, 'tools', 'polyhaven_authors.json')
LICENCE = {'by': 'CC BY 4.0', 'cc0': 'CC0'}


def polyhaven(ids):
    cache = json.load(open(CACHE, encoding='utf-8')) if os.path.exists(CACHE) else {}
    for i in sorted(set(ids) - set(cache)):
        try:
            req = urllib.request.Request(f'https://api.polyhaven.com/info/{i}', headers={'User-Agent': 'MapSmithEE credits'})
            info = json.load(urllib.request.urlopen(req, timeout=20))
            cache[i] = {'name': info.get('name', i), 'authors': sorted(info.get('authors', {}))}
        except Exception:
            cache[i] = None   # not a Poly Haven asset
    json.dump(cache, open(CACHE, 'w', encoding='utf-8'), indent=1, ensure_ascii=False)
    return cache


def main():
    shipped = json.load(open(os.path.join(LIB, 'models.json'), encoding='utf-8'))
    meta = json.load(open(META, encoding='utf-8'))
    items = [meta[k] for k in shipped if k in meta]
    sketchfab, other, ph_ids = {}, set(), set()
    for it in items:
        c, src = it.get('credit'), it.get('src', {})
        if isinstance(c, dict):
            sketchfab[c['uid']] = c
        elif isinstance(c, str):
            other.add(c)
        elif 'asset' in src:
            ph_ids.add(src['asset'])
    tex = [f.rsplit('.', 1)[0] for f in sorted(os.listdir(os.path.join(LIB, 'tex')))]
    ph_ids |= {t for t in tex if not t.startswith(('acifer_', 'fire_'))}
    ph = polyhaven(ph_ids)

    out = ['# Credits', '',
           'MapSmithEE ships the Camp 3D model library and an example map. Thank you to every author below.', '',
           '## Poly Haven (CC0)', '', 'https://polyhaven.com - free CC0 models and textures.', '']
    for i in sorted(ph):
        if ph[i]:
            out.append(f"- {ph[i]['name']} (`{i}`) by {', '.join(ph[i]['authors'])}, https://polyhaven.com/a/{i}")
    out += ['', '## Sketchfab / Objaverse models', '',
            'Found through the Objaverse index (https://objaverse.allenai.org). CC BY 4.0 models require this credit.', '']
    for c in sorted(sketchfab.values(), key=lambda c: (c['name'].lower(), c['author'].lower())):
        out.append(f"- \"{c['name']}\" by {c['author']} ({LICENCE.get(c['license'], c['license'])}), "
                   f"https://sketchfab.com/3d-models/{c['uid']}")
    if other:
        out += ['', '## Other pieces', ''] + [f'- {c}' for c in sorted(other)]
    out += ['', "## Baldur's Gate art (non-commercial fan content only)", '',
            "- BG1 tree cut-outs (`bg_*` pieces): BioWare's art from Baldur's Gate, templates shared by Gwendolyne "
            'on the Beamdog forums (https://forums.beamdog.com/discussion/71485).',
            "- Ground textures `acifer_*`: by Acifer, rebuilt from the BG series' areas "
            '(https://www.gibberlings3.net/forums/topic/38346-baldurs-gate-textures-for-your-3d-renders/). '
            'Non-commercial fan content for the Infinity Engine games only.',
            "- Fire pit flames (`fire_FPIT1*`): frames of the game's own FPIT1 animation (BioWare / Beamdog).", '']
    open(os.path.join(ROOT, 'CREDITS.md'), 'w', encoding='utf-8').write('\n'.join(out))
    print(f'{len(ph)} Poly Haven ids ({sum(1 for v in ph.values() if v)} found), {len(sketchfab)} Sketchfab models')


main()
