# Render a Camp Map 3D layout the way BG pre-rendered its areas: full models, painted ground, sun of the chosen hour,
# orthographic camera looking north 38° down, 60 px per metre. Usage:
#   blender -b -P render_map.py -- <layout.json> <ground.json> <out.png> [samples] [time] [x0 y0 x1 y1 (px crop)]
# layout.json / ground.json: the editor's maps/camp3d and maps/camp3d_ground documents.
# Run by MapSmithEE (src-tauri/src/camprender), which sets MAPSMITH_LIBRARY / MAPSMITH_WORK (see paths.py), and
# MAPSMITH_CANVAS (a JSON file: item index -> its tent / rug canvas PNG, made beforehand: Blender's Python has no PIL).
# Lines starting with 'MAPSMITH ' are progress for MapSmithEE.
import bpy, sys, os, json, math, base64
from mathutils import Vector, Quaternion
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.dont_write_bytecode = True   # the scripts may sit in a read-only install folder
import norm
from paths import LIB, WORK, TEX, lib as libpath
a = sys.argv[sys.argv.index('--') + 1:]
L, G, OUT = json.load(open(a[0])), json.load(open(a[1])), a[2]
L, G = L.get('data', L), G.get('data', G)
if G.get('split'):
    # the editor saves one mask per document next to the ground one: maps/<id>_g<n>
    base = a[1][:-len('_ground.json')]
    G['masks'] = [(lambda f: (lambda d: d.get('data', d).get('mask'))(json.load(open(f))) if os.path.exists(f) else None)(f'{base}_g{i}.json') for i in range(len(G['layers']))]
SAMPLES = int(a[3]) if len(a) > 3 else 128
HOUR = float(a[4]) if len(a) > 4 and a[4] != '-' else L.get('time', 14)
# several pictures from one loaded scene (MapSmithEE: night, then day with its passes): MAPSMITH_JOBS =
# [{"out": png, "hour": h, "id": bool}, ...]; without it, the one picture of the command line
JOBS = json.loads(os.environ['MAPSMITH_JOBS']) if os.environ.get('MAPSMITH_JOBS') else [{'out': OUT, 'hour': HOUR, 'id': bool(os.environ.get('BG3D_IDPASS'))}]
JOBS.sort(key=lambda j: bool(j.get('id')))   # the id passes change the render settings: last
CROP = [int(v) for v in a[5:9]] if len(a) > 8 else None
MODELS = json.load(open(f'{LIB}/models.json'))
PROXY_DIMS = {}
for _f in os.listdir(f'{LIB}/proxies'):
    if _f.startswith('index') and _f.endswith('.json'): PROXY_DIMS.update({k: v['dims'] for k, v in json.load(open(f'{LIB}/proxies/{_f}')).items()})
# the user's own pieces and ground textures (MapSmithEE "Import...": MAPSMITH_USERLIB), next to the library's
USERLIB = os.environ.get('MAPSMITH_USERLIB', '').replace(chr(92), '/').rstrip('/')
if USERLIB and os.path.exists(f'{USERLIB}/models.json'):
    MODELS.update(json.load(open(f'{USERLIB}/models.json')))
    if os.path.exists(f'{USERLIB}/proxies/index_user.json'):
        PROXY_DIMS.update({k: v['dims'] for k, v in json.load(open(f'{USERLIB}/proxies/index_user.json')).items()})
def texdir(name): return f'{USERLIB}/tex/{name}' if USERLIB and os.path.isdir(f'{USERLIB}/tex/{name}') else f'{TEX}/{name}'
# per-model height corrections from the editor's Calibrate mode (db models/offsets, exported next to the layout)
OFF_F = os.path.join(os.path.dirname(a[0]), 'models', 'offsets.json')
OFFSETS = (lambda d: d.get('data', d).get('offsets', {}))(json.load(open(OFF_F))) if os.path.exists(OFF_F) else {}
W, D = L['size_m']; ELEV = math.radians(L.get('elev', 38))
# the game's scale (BG3D_PXM, 42 px/m: BG's characters); the image size follows from the map's size in metres
PXM = float(os.environ.get('BG3D_PXM', L.get('px_per_m', 60)))
MAPW, MAPH = round(W * PXM / 64) * 64, round(D * PXM * math.sin(ELEV) / 64) * 64
# HD tiles (IEX): the pictures at SCALE x that size, same camera; the alpha and id passes stay 1x (walls are 1x)
SCALE = int(os.environ.get('MAPSMITH_SCALE') or 1)

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = 'CYCLES'
prefs = bpy.context.preferences.addons['cycles'].preferences
# OptiX first (the reference renders), else the first GPU backend this machine has, else the CPU; MAPSMITH_DEVICE=cpu:
# the CPU only (MapSmithEE asks for it after a graphics card ran out of memory, or when the user ticks it)
for _kind in (() if os.environ.get('MAPSMITH_DEVICE') == 'cpu' else ('OPTIX', 'CUDA', 'HIP', 'ONEAPI', 'METAL')):
    try: prefs.compute_device_type = _kind
    except TypeError: continue
    prefs.get_devices()
    if any(d.type == _kind for d in prefs.devices): break
_cpu = os.environ.get('MAPSMITH_DEVICE') == 'cpu'
for d in prefs.devices: d.use = True
sc.cycles.device = 'GPU' if not _cpu and any(d.type != 'CPU' for d in prefs.devices) else 'CPU'
sc.cycles.samples = SAMPLES; sc.cycles.use_denoising = True
print('MAPSMITH device', prefs.compute_device_type, sc.cycles.device, flush=True)
sc.view_settings.view_transform = 'Standard'; sc.view_settings.exposure = -0.4
# Cycles' status ('... | Updating Images', '... | Sample 3/128'): bpy.ops.render.render prints none of it by itself
bpy.app.handlers.render_stats.append(lambda st: print('MAPSMITH stats', st, flush=True))

def three_to_bl(p): return Vector((p[0], -p[2], p[1]))     # editor (x, y up, z south) -> Blender (x, y north, z up)

# ---------- ground: the editor's layers, each texture kept where its painted mask says ----------
bpy.ops.mesh.primitive_plane_add(size=1, location=(W / 2, -D / 2, 0)); gnd = bpy.context.object
gnd.scale = (W, D, 1); bpy.ops.object.transform_apply(scale=True)
mat = bpy.data.materials.new('ground'); mat.use_nodes = True; nt = mat.node_tree; p = nt.nodes['Principled BSDF']
p.inputs['Roughness'].default_value = 1.0
uv = nt.nodes.new('ShaderNodeTexCoord')
os.makedirs(f'{WORK}/masks', exist_ok=True)
TEXFILES = json.loads(os.environ.get('BG3D_TEXFILES') or '{}')   # recoloured layers (render_scene.py)
TEXM_BY = G.get('tex_m_by', {})                                  # metres per image, per layer (BG-pixel textures differ)
def mix(a, b, fac, kind='RGBA'):
    m = nt.nodes.new('ShaderNodeMix'); m.data_type = kind
    ins = [x for x in m.inputs if x.type == ('RGBA' if kind == 'RGBA' else 'VALUE') and x.name != 'Factor']
    nt.links.new(fac, m.inputs['Factor']); nt.links.new(a, ins[0]); nt.links.new(b, ins[1])
    return [x for x in m.outputs if x.type == ('RGBA' if kind == 'RGBA' else 'VALUE')][0]
col = None; hgt = None
for i, name in enumerate(G['layers']):
    tm = TEXM_BY.get(name, G.get('tex_m', 4))
    name = name.split('~')[0]   # a variant ("dirt~2": its own colour) uses its base layer's texture
    tile = nt.nodes.new('ShaderNodeMapping'); tile.inputs['Scale'].default_value = (W / tm, D / tm, 1)
    nt.links.new(uv.outputs['UV'], tile.inputs[0])
    t = nt.nodes.new('ShaderNodeTexImage'); t.image = bpy.data.images.load(TEXFILES.get(str(i)) or f'{texdir(name)}/diff_2k.jpg'); nt.links.new(tile.outputs[0], t.inputs[0])
    # relief: the layer's bump / displacement map, else flat
    hp = next((f for f in (f'{texdir(name)}/bump.jpg', f'{texdir(name)}/disp_2k.jpg') if os.path.exists(f)), None)
    if hp:
        hn = nt.nodes.new('ShaderNodeTexImage'); hn.image = bpy.data.images.load(hp); hn.image.colorspace_settings.name = 'Non-Color'
        nt.links.new(tile.outputs[0], hn.inputs[0]); h_out = hn.outputs[0]
    else:
        v = nt.nodes.new('ShaderNodeValue'); v.outputs[0].default_value = 0.5; h_out = v.outputs[0]
    if i == 0: col, hgt = t.outputs[0], h_out; continue
    url = G['masks'][i]
    if not url: continue
    path = f"{WORK}/masks/m{i}.{'webp' if url.startswith('data:image/webp') else 'png'}"; open(path, 'wb').write(base64.b64decode(url.split(',', 1)[1]))
    mk = nt.nodes.new('ShaderNodeTexImage'); mk.image = bpy.data.images.load(path); mk.image.colorspace_settings.name = 'Non-Color'
    nt.links.new(uv.outputs['UV'], mk.inputs[0]); mk.interpolation = 'Cubic'
    col = mix(col, t.outputs[0], mk.outputs['Alpha'])
    hgt = mix(hgt, h_out, mk.outputs['Alpha'], 'FLOAT')
# mask rows run north to south like the image's rows, and Blender's v = 1 (top row) is the plane's north edge: no flip
nt.links.new(col, p.inputs['Base Color'])
bump = nt.nodes.new('ShaderNodeBump'); bump.inputs['Strength'].default_value = 0.35; bump.inputs['Distance'].default_value = 0.05
nt.links.new(hgt, bump.inputs['Height']); nt.links.new(bump.outputs['Normal'], p.inputs['Normal'])
gnd.data.materials.append(mat)

# ---------- water: where the painted ground is water (the layers whose texture is named ...water..., and their
# variants), in the ground's own coordinates (rows north to south: the BG camera maps them straight onto the picture),
# for the area build's water overlay (campbuild/water.rs), next to the pictures: water_mask.png (white = water), and the
# texture of the water layer covering most as water_tex.png (64 x 64, seamless) ----------
OUTDIR = os.path.dirname(os.path.abspath(JOBS[0]['out']))
def write_water():
    import numpy as np
    for f in ('water_mask.png', 'water_tex.png'):
        if os.path.exists(f'{OUTDIR}/{f}'): os.remove(f'{OUTDIR}/{f}')
    layers = G['layers']
    is_water = [n.split('~')[0].lower().find('water') >= 0 for n in layers]
    if not any(is_water): return
    alphas, size = {}, None
    for i in range(1, len(layers)):
        url = G['masks'][i]
        if not url: continue
        f = f"{WORK}/masks/m{i}.{'webp' if url.startswith('data:image/webp') else 'png'}"
        im = bpy.data.images.load(f); w_, h_ = im.size
        a = np.empty(w_ * h_ * 4, np.float32); im.pixels.foreach_get(a); bpy.data.images.remove(im)
        alphas[i] = a.reshape(h_, w_, 4)[::-1, :, 3]; size = size or (h_, w_)
    size = size or (64, 64)
    cover = np.full(size, 1.0 if is_water[0] else 0.0, np.float32)
    share = {0: float(is_water[0])}
    for i, a in alphas.items():
        if a.shape != size: continue
        cover = cover * (1 - a) + a * is_water[i]
        if is_water[i]: share[i] = float(a.mean())
    if cover.max() < 0.5: return
    rgba = np.ones(size + (4,), np.float32); rgba[..., 0] = rgba[..., 1] = rgba[..., 2] = cover
    im = bpy.data.images.new('water_mask', size[1], size[0], alpha=True); im.pixels.foreach_set(rgba[::-1].ravel())
    im.filepath_raw = f'{OUTDIR}/water_mask.png'; im.file_format = 'PNG'; im.save(); bpy.data.images.remove(im)
    k = max(share, key=share.get); name = layers[k].split('~')[0]
    tex = bpy.data.images.load(TEXFILES.get(str(k)) or f'{texdir(name)}/diff_2k.jpg')
    tex.scale(64, 64); tex.filepath_raw = f'{OUTDIR}/water_tex.png'; tex.file_format = 'PNG'; tex.save(); bpy.data.images.remove(tex)
    print('water', name, 'covers', round(float((cover >= 0.5).mean()) * 100, 1), '% of the ground', flush=True)
write_water()

# ---------- models: one full copy per kind, linked duplicates for every placement ----------
lib = bpy.data.collections.new('lib'); sc.collection.children.link(lib)
lay = sc.view_layers[0].layer_collection.children['lib']; lay.exclude = True
proto = {}
CANVAS = json.load(open(os.environ['MAPSMITH_CANVAS'])) if os.environ.get('MAPSMITH_CANVAS') else {}
insts = []   # (item index, object) for the id pass
decals = []  # BG trees' painted shadows: not pieces, left out of the coverage and id passes
SIN, COS = math.sin(ELEV), math.cos(ELEV)
_day = max(0, math.sin((HOUR - 6) / 12 * math.pi)); _night = HOUR < 6.5 or HOUR > 20.5
def sprite_k(h):   # undo the scene's -0.4 exposure; dim by night
    d = max(0, math.sin((h - 6) / 12 * math.pi)); return 2 ** 0.4 * (0.3 if (h < 6.5 or h > 20.5) else min(1.0, 0.6 + d * 0.6))
SPRITE_K = sprite_k(HOUR)
SPRITE_EMS = []   # BG trees' emission nodes: their strength follows each picture's hour
_bgmesh = {}
# ---------- a piece's own colour (item 'col' = {h, s, b, c}): the editor's CSS filter maths (hue-rotate, saturate,
# brightness, contrast on sRGB values) as a node group spliced in front of the colour the material shows ----------
_colgroups = {}
def _rows_hue(deg):
    c, s_ = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return [(0.213 + c * 0.787 - s_ * 0.213, 0.715 - c * 0.715 - s_ * 0.715, 0.072 - c * 0.072 + s_ * 0.928),
            (0.213 - c * 0.213 + s_ * 0.143, 0.715 + c * 0.285 + s_ * 0.140, 0.072 - c * 0.072 - s_ * 0.283),
            (0.213 - c * 0.213 - s_ * 0.787, 0.715 - c * 0.715 + s_ * 0.715, 0.072 + c * 0.928 + s_ * 0.072)]
def _rows_sat(v):
    return [(0.213 + 0.787 * v, 0.715 - 0.715 * v, 0.072 - 0.072 * v), (0.213 - 0.213 * v, 0.715 + 0.285 * v, 0.072 - 0.072 * v),
            (0.213 - 0.213 * v, 0.715 - 0.715 * v, 0.072 + 0.928 * v)]
def colgroup(col):
    key = json.dumps(col, sort_keys=True)
    if key in _colgroups: return _colgroups[key]
    g = bpy.data.node_groups.new('piece_colour', 'ShaderNodeTree')
    g.interface.new_socket('Color', in_out='INPUT', socket_type='NodeSocketColor')
    g.interface.new_socket('Color', in_out='OUTPUT', socket_type='NodeSocketColor')
    n = g.nodes; L_ = g.links
    gi, go = n.new('NodeGroupInput'), n.new('NodeGroupOutput')
    def vm(op, a, b=None, val=None):
        m = n.new('ShaderNodeVectorMath'); m.operation = op; L_.new(a, m.inputs[0])
        if b is not None: L_.new(b, m.inputs[1])
        if val is not None:
            if op == 'SCALE': m.inputs['Scale'].default_value = val
            else: m.inputs[1].default_value = val
        return m.outputs['Vector'] if op != 'DOT_PRODUCT' else m.outputs['Value']
    def clamp01(v): return vm('MAXIMUM', vm('MINIMUM', v, val=(1, 1, 1)), val=(0, 0, 0))
    def mat3(v, rows):
        cx = n.new('ShaderNodeCombineXYZ')
        for i, r in enumerate(rows): L_.new(vm('DOT_PRODUCT', v, val=r), cx.inputs[i])
        return clamp01(cx.outputs['Vector'])
    gam = n.new('ShaderNodeGamma'); gam.inputs['Gamma'].default_value = 1 / 2.2; L_.new(gi.outputs[0], gam.inputs['Color'])
    v = mat3(gam.outputs[0], _rows_hue(col.get('h', 0)))
    v = mat3(v, _rows_sat(col.get('s', 1)))
    v = clamp01(vm('SCALE', v, val=col.get('b', 1)))
    v = clamp01(vm('ADD', vm('SCALE', vm('SUBTRACT', v, val=(0.5, 0.5, 0.5)), val=col.get('c', 1)), val=(0.5, 0.5, 0.5)))
    back = n.new('ShaderNodeGamma'); back.inputs['Gamma'].default_value = 2.2; L_.new(v, back.inputs['Color']); L_.new(back.outputs[0], go.inputs[0])
    _colgroups[key] = g; return g
def recolour(ob, col):
    """this piece's own copies of its materials, each with the colour group in front of its Base Color / emission"""
    if not col or (not col.get('h') and col.get('s', 1) == 1 and col.get('b', 1) == 1 and col.get('c', 1) == 1): return
    g = colgroup(col)
    for slot in ob.material_slots:
        if not slot.material or not slot.material.use_nodes: continue
        m = slot.material.copy(); slot.link = 'OBJECT'; slot.material = m; nt_ = m.node_tree
        for node in list(nt_.nodes):
            sock = node.inputs.get('Base Color') if node.type == 'BSDF_PRINCIPLED' else node.inputs.get('Color') if node.type == 'EMISSION' else None
            if not sock or not sock.is_linked: continue
            src = sock.links[0].from_socket
            gn = nt_.nodes.new('ShaderNodeGroup'); gn.node_tree = g
            nt_.links.new(src, gn.inputs[0]); nt_.links.new(gn.outputs[0], sock)
def bgsprite(mid, B, s):
    """BG's own tree: a cut-out facing the camera, exactly its size in game pixels at this map's scale, and its
    painted shadow on the ground (the twin of tools/bgsprites.py's editor copy)."""
    if mid not in _bgmesh:
        (w, h), (fx, fy) = B['size'], B['foot']
        corners = [(0, 0), (w, 0), (w, h), (0, h)]
        tree = [((x - fx) / PXM, SIN * (fy - y) / PXM, COS * (fy - y) / PXM) for x, y in corners]   # Blender: up = (0, sin, cos)
        shad = [((x - fx) / PXM, -(y - fy) / (PXM * SIN), 0.015) for x, y in corners]
        out = []
        for kind, verts in (('tree', tree), ('shadow', shad)):
            me = bpy.data.meshes.new(f'{mid}_{kind}'); me.from_pydata(verts, [], [(0, 3, 2, 1)])
            uvl = me.uv_layers.new()
            for li, vi in enumerate(me.polygons[0].vertices): x, y = corners[vi]; uvl.data[li].uv = (x / w, 1 - y / h)
            m = bpy.data.materials.new(f'{mid}_{kind}'); m.use_nodes = True; n = m.node_tree; n.nodes.clear()
            o_ = n.nodes.new('ShaderNodeOutputMaterial'); tx = n.nodes.new('ShaderNodeTexImage'); tx.image = bpy.data.images.load(libpath(B[kind]))
            mx = n.nodes.new('ShaderNodeMixShader'); tr = n.nodes.new('ShaderNodeBsdfTransparent')
            if kind == 'tree':   # pre-lit art: emitted as painted
                em = n.nodes.new('ShaderNodeEmission'); em.inputs['Strength'].default_value = SPRITE_K; SPRITE_EMS.append(em)
                n.links.new(tx.outputs['Color'], em.inputs['Color']); n.links.new(tx.outputs['Alpha'], mx.inputs['Fac'])
                n.links.new(tr.outputs[0], mx.inputs[1]); n.links.new(em.outputs[0], mx.inputs[2])
            else:                # the shadow darkens the ground beneath it
                k = n.nodes.new('ShaderNodeMath'); k.operation = 'MULTIPLY'; k.inputs[1].default_value = 0.55
                df = n.nodes.new('ShaderNodeBsdfDiffuse'); df.inputs['Color'].default_value = (0, 0, 0, 1)
                n.links.new(tx.outputs['Alpha'], k.inputs[0]); n.links.new(k.outputs[0], mx.inputs['Fac'])
                n.links.new(tr.outputs[0], mx.inputs[1]); n.links.new(df.outputs[0], mx.inputs[2])
            n.links.new(mx.outputs[0], o_.inputs['Surface']); me.materials.append(m); out.append(me)
        _bgmesh[mid] = out
    objs = []
    for me in _bgmesh[mid]:
        ob = bpy.data.objects.new(me.name, me); sc.collection.objects.link(ob)
        ob.scale = (s, s, s); ob.visible_shadow = False; objs.append(ob)
    return objs
# progress: loading a model's full copy is what takes the time (seconds to minutes each), placing a copy is quick
_kinds = len({it['m'] for it in L['items'] if it['m'] in MODELS and 'bgsprite' not in MODELS[it['m']]['src']})
for idx, it in enumerate(L['items']):
    mid = it['m']
    print(f"MAPSMITH item {idx + 1}/{len(L['items'])}", flush=True)
    if mid not in MODELS: continue
    if 'bgsprite' in MODELS[mid]['src']:
        tree_o, shad_o = bgsprite(mid, MODELS[mid]['src']['bgsprite'], it.get('s', 1))
        tree_o.location = shad_o.location = three_to_bl(it['p'])
        recolour(tree_o, it.get('col')); insts.append((idx, tree_o)); decals.append(shad_o); continue
    if mid not in proto:
        print(f"MAPSMITH models {len(proto)}/{_kinds} {mid} {MODELS[mid].get('label', '')}", flush=True)
        c = bpy.data.collections.new(mid); lib.children.link(c)
        ob = norm.load(MODELS[mid], mid)
        for col_ in list(ob.users_collection): col_.objects.unlink(ob)
        c.objects.link(ob); proto[mid] = c
    s, sz = it.get('s', 1), it.get('sz', [1, 1, 1])
    look = it.get('tent') or MODELS[mid].get('canvas')
    if look:
        # a tent gets its own copy (mesh shared) whose canvas slot carries this tent's pattern image
        src = proto[mid].objects[0]; inst = src.copy(); sc.collection.objects.link(inst)
        img = bpy.data.images.load(CANVAS[str(idx)], check_existing=True)
        for k, slot in enumerate(inst.material_slots):
            if slot.material and slot.material.name.startswith('canvas'):
                m = slot.material.copy(); slot.link = 'OBJECT'; slot.material = m
                for n in m.node_tree.nodes:
                    if n.type == 'TEX_IMAGE' and n.image and n.image.name.startswith('white'): n.image = img
    else:
        # a linked copy (mesh shared) rather than a collection instance: the id pass below needs one object per piece
        inst = proto[mid].objects[0].copy(); sc.collection.objects.link(inst)
    q = it.get('q', [0, 0, 0, 1])                       # three.js (x, y, z, w) in its axes
    inst.rotation_mode = 'QUATERNION'; inst.rotation_quaternion = Quaternion((q[3], q[0], -q[2], q[1]))
    inst.location = three_to_bl(it['p'])
    inst.scale = (s * sz[0], s * sz[2], s * sz[1])     # editor (width, height, depth) -> Blender (x, y depth, z up)
    # calibrated height, minus a tree's root flare (models.json 'sink'): the trunk comes straight out of the ground like BG's
    inst.location.z += (OFFSETS.get(mid, 0) - MODELS[mid].get('sink', 0)) * s * sz[1]
    recolour(inst, it.get('col'))
    insts.append((idx, inst))
print('placed', len(L['items']), 'items,', len(proto), 'models', flush=True)

# ---------- light: the editor's sun for that hour (rebuilt for each picture) ----------
_lights = []
def light(h):
    for o in _lights: bpy.data.objects.remove(o, do_unlink=True)
    _lights.clear()
    day = max(0, math.sin((h - 6) / 12 * math.pi))
    # BG's own light (measured on BG1's trees): south-west at 45°, all day; the hour sets colour and strength only
    _hz = Vector((-0.61, 0, 0.79)).normalized() * math.cos(math.pi / 4)
    d3 = Vector((_hz.x, math.sin(math.pi / 4), _hz.z))   # towards the sun, editor axes
    dB = three_to_bl(d3)
    night = h < 6.5 or h > 20.5
    bpy.ops.object.light_add(type='SUN'); sun = bpy.context.object; _lights.append(sun)
    sun.rotation_euler = dB.to_track_quat('Z', 'Y').to_euler(); sun.data.angle = math.radians(6)   # soft edges, like BG's painted shadows
    sun.data.energy = 0.35 if night else 1.0 + day * 3.2
    sun.data.color = (0.56, 0.65, 0.85) if night else ((1.0, 0.7, 0.45) if day < 0.35 else (1.0, 0.95, 0.86))
    # fires, braziers, lanterns and torches light the camp (the editor's FIRES table: height share, power): strongly at
    # night, a faint warm halo round their hearths by day (John 2026-11-02)
    # (share of the piece's height, power in W, radius of the light in m: a big source casts soft shadows, as a real
    # metre-wide camp fire does; one wide light, not a ring of small ones, which would cast a star of sharp shadows)
    FIRES = {'campfire': (0.4, 2700, 0.9), 'brazier': (0.9, 900, 0.35), 'forge': (0.8, 900, 0.35), 'torch': (0.85, 400, 0.25),
             'lantern': (0.7, 150, 0.15), 'lamp post': (0.9, 300, 0.2)}
    DAYK = 1.0 if night else 0.05
    nf = 0
    for it in L['items']:
        m = MODELS.get(it['m']); f = m and FIRES.get(m['label'])
        if not f: continue
        s_ = it.get('s', 1); dims = PROXY_DIMS.get(it['m'], [1, 1, 1])
        bpy.ops.object.light_add(type='POINT'); l = bpy.context.object; _lights.append(l)
        # at least 0.7 m up: a light sitting in the pit burns its stones white
        l.location = three_to_bl(it['p']) + Vector((0, 0, max(0.7, f[0] * dims[2] * s_)))
        l.data.energy = f[1] * s_ * DAYK; l.data.color = (1.0, 0.52, 0.2); l.data.shadow_soft_size = f[2] * s_; nf += 1
    print('fire lights', nf, flush=True)
    # the user's own lights (the editor's 'light' pieces): colour (sRGB hex), watts, size, always or by night only
    lin = lambda v: v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    nl = 0
    for it in L['items']:
        if it['m'] != 'light' or (it.get('when') == 'night' and not night): continue
        hx = (it.get('lc') or '#ffc98a').lstrip('#')
        bpy.ops.object.light_add(type='POINT'); l = bpy.context.object; _lights.append(l)
        l.location = three_to_bl(it['p'])
        l.data.energy = float(it.get('w') or 300); l.data.shadow_soft_size = float(it.get('r', 0.15))
        l.data.color = tuple(lin(int(hx[i:i + 2], 16) / 255) for i in (0, 2, 4)); nl += 1
    print('own lights', nl, flush=True)
    w = bpy.data.worlds.new('w'); sc.world = w; w.use_nodes = True
    w.node_tree.nodes['Background'].inputs[0].default_value = (0.04, 0.05, 0.1, 1) if night else (0.5, 0.56, 0.68, 1)
    w.node_tree.nodes['Background'].inputs[1].default_value = 0.35 if night else 0.9
    for em in SPRITE_EMS: em.inputs['Strength'].default_value = sprite_k(h)

# ---------- camera: BG's ----------
bpy.ops.object.camera_add(); cam = bpy.context.object; sc.camera = cam
cam.data.type = 'ORTHO'; cam.rotation_euler = (math.pi / 2 - ELEV, 0, 0)
ctr = Vector((W / 2, -D / 2, 0)); cam.location = ctr + Vector((0, -100 * math.cos(ELEV), 100 * math.sin(ELEV)))
cam.data.ortho_scale = MAPW / PXM; cam.data.clip_end = 1000
sc.render.resolution_x, sc.render.resolution_y = MAPW, MAPH
if CROP:
    sc.render.use_border = True; sc.render.use_crop_to_border = True
    x0, y0, x1, y1 = CROP
    sc.render.border_min_x, sc.render.border_max_x = x0 / MAPW, x1 / MAPW
    sc.render.border_min_y, sc.render.border_max_y = 1 - y1 / MAPH, 1 - y0 / MAPH
sc.render.image_settings.file_format = 'PNG'
# "Export scene" (MAPSMITH_EXPORT, see exportscene.py): the scene lit for its hour, saved instead of rendered
if os.environ.get('MAPSMITH_EXPORT'):
    import exportscene
    light(HOUR)
    exportscene.save(json.loads(os.environ['MAPSMITH_EXPORT']), sc, gnd, lib, list(_bgmesh), PXM)
    sys.exit(0)
# ---------- light pass (area builds): the direct and indirect light every surface gets (no surface colour), saved by
# the compositor during the picture's own render, then turned into <out>_light.png at the game's 1x size for the light
# maps (LM by day, LN by night: campbuild::light). RGB = direct light, open ground in full sun (or moon) = LIGHT_REF, so
# fires keep their colour up to 8 times brighter; A = 255 where a surface takes light at all (0 under BG sprite cards,
# which are lit by their own emission) ----------
LIGHT_REF = 32
def light_setup(out):
    vl = sc.view_layers[0]; on = bool(out)
    vl.use_pass_diffuse_direct = vl.use_pass_diffuse_indirect = on; sc.render.use_compositing = on
    if not on: return
    ng = bpy.data.node_groups.new('mapsmith_light', 'CompositorNodeTree'); sc.compositing_node_group = ng
    ng.interface.new_socket('Image', in_out='OUTPUT', socket_type='NodeSocketColor')
    rl = ng.nodes.new('CompositorNodeRLayers'); go = ng.nodes.new('NodeGroupOutput'); ng.links.new(rl.outputs['Image'], go.inputs[0])
    fo = ng.nodes.new('CompositorNodeOutputFile'); fo.directory = os.path.dirname(out); fo.file_name = os.path.basename(out)[:-4] + '_light_'
    fo.format.media_type = 'IMAGE'; fo.format.file_format = 'OPEN_EXR'; fo.format.color_depth = '32'
    for nm in ('Diffuse Direct', 'Diffuse Indirect'):
        it = fo.file_output_items.new('RGBA', nm.replace(' ', '')); ng.links.new(rl.outputs[nm], fo.inputs[it.name])
def light_write(out, night):
    import numpy as np
    passes = []
    for nm in ('DiffuseDirect', 'DiffuseIndirect'):
        f = out[:-4] + '_light_' + nm + '.exr'; im = bpy.data.images.load(f); w, h = im.size
        a = np.empty(w * h * 4, np.float32); im.pixels.foreach_get(a); bpy.data.images.remove(im); os.remove(f)
        a = a.reshape(h, w, 4)[::-1, :, :3]   # Blender's rows go bottom-up
        if SCALE > 1: a = a.reshape(h // SCALE, SCALE, w // SCALE, SCALE, 3).mean((1, 3))
        passes.append(a)
    d, ind = passes; h, w = d.shape[:2]
    lum = lambda x: x @ np.array([0.2126, 0.7152, 0.0722], np.float32)
    # the reference: most of the lit pixels are flat ground in full sun; by night, in moonlight only (bluish: the fires'
    # orange light would otherwise set it on a small map)
    ld = lum(d); lit = ld[(ld > 1e-4) & ((d[..., 2] >= d[..., 0]) if night else True)]
    ref = float(np.percentile(lit, 75)) if lit.size else 1.0
    rgba = np.empty((h, w, 4), np.float32)
    rgba[..., :3] = np.clip(d / max(ref, 1e-6) * LIGHT_REF, 0, 255).round() / 255
    rgba[..., 3] = (ld + lum(ind)) > 1e-4
    im = bpy.data.images.new('light', w, h, alpha=True); im.pixels.foreach_set(rgba[::-1].ravel())
    im.filepath_raw = out[:-4] + '_light.png'; im.file_format = 'PNG'; im.save(); bpy.data.images.remove(im)
    print('rendered light', out[:-4] + '_light.png', flush=True)
for k, job in enumerate(JOBS):
    OUT = job['out']; light(float(job['hour']) if job.get('hour') is not None else HOUR)
    sc.render.resolution_percentage = 100 * SCALE
    print(f"MAPSMITH job {k + 1}/{len(JOBS)}", flush=True)
    sc.render.filepath = OUT
    light_setup(OUT if job.get('light') else None)
    print('MAPSMITH stage render', flush=True)
    bpy.ops.render.render(write_still=True)
    print('rendered', OUT, flush=True)
    if job.get('light'):
        h_ = float(job['hour']) if job.get('hour') is not None else HOUR
        light_write(OUT, h_ < 6.5 or h_ > 20.5); light_setup(None)
    if not job.get('id'): continue
    # 1) coverage: the pieces alone (ground hidden), transparent background; the alpha keeps leaf cards' real holes
    for d_ in decals: d_.hide_render = True
    gnd.hide_render = True; sc.render.film_transparent = True; sc.cycles.samples = 16; sc.cycles.use_denoising = False
    sc.render.image_settings.color_mode = 'RGBA'
    sc.render.resolution_percentage = 100
    print('MAPSMITH stage alpha', flush=True)
    sc.render.filepath = OUT[:-4] + '_alpha.png'; bpy.ops.render.render(write_still=True)
    gnd.hide_render = False; sc.render.film_transparent = False
    # 2) id pass: every piece in a flat colour that encodes its item index + 1, the ground black; no antialiasing, so
    # the game build can cut each piece's silhouette (id AND coverage) into a wall polygon
    sc.render.engine = 'BLENDER_WORKBENCH'
    sc.display.shading.light = 'FLAT'; sc.display.shading.color_type = 'OBJECT'
    sc.display.render_aa = 'OFF'; sc.view_settings.view_transform = 'Raw'; sc.view_settings.exposure = 0   # Raw: the colours stay exact numbers
    sc.render.dither_intensity = 0   # dither adds +-1 noise: a piece's number then read as its neighbour's (fire pit pixels in the tent's walls)
    sc.display.shading.show_shadows = False; sc.display.shading.show_cavity = False
    gnd.color = (0, 0, 0, 1)
    for idx, inst in insts:
        v = idx + 1; inst.color = (((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255, 1)
    sc.render.image_settings.color_depth = '8'; sc.render.image_settings.color_mode = 'RGB'
    print('MAPSMITH stage id', flush=True)
    sc.render.filepath = OUT[:-4] + '_id.png'; bpy.ops.render.render(write_still=True)
    print('rendered id', sc.render.filepath, flush=True)
