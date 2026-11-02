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
CROP = [int(v) for v in a[5:9]] if len(a) > 8 else None
MODELS = json.load(open(f'{LIB}/models.json'))
PROXY_DIMS = {}
for _f in os.listdir(f'{LIB}/proxies'):
    if _f.startswith('index') and _f.endswith('.json'): PROXY_DIMS.update({k: v['dims'] for k, v in json.load(open(f'{LIB}/proxies/{_f}')).items()})
# per-model height corrections from the editor's Calibrate mode (db models/offsets, exported next to the layout)
OFF_F = os.path.join(os.path.dirname(a[0]), 'models', 'offsets.json')
OFFSETS = (lambda d: d.get('data', d).get('offsets', {}))(json.load(open(OFF_F))) if os.path.exists(OFF_F) else {}
W, D = L['size_m']; ELEV = math.radians(L.get('elev', 38))
# the game's scale (BG3D_PXM, 42 px/m: BG's characters); the image size follows from the map's size in metres
PXM = float(os.environ.get('BG3D_PXM', L.get('px_per_m', 60)))
MAPW, MAPH = round(W * PXM / 64) * 64, round(D * PXM * math.sin(ELEV) / 64) * 64

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = 'CYCLES'
prefs = bpy.context.preferences.addons['cycles'].preferences
# OptiX first (the reference renders), else the first GPU backend this machine has, else the CPU
for _kind in ('OPTIX', 'CUDA', 'HIP', 'ONEAPI', 'METAL'):
    try: prefs.compute_device_type = _kind
    except TypeError: continue
    prefs.get_devices()
    if any(d.type == _kind for d in prefs.devices): break
for d in prefs.devices: d.use = True
sc.cycles.device = 'GPU' if any(d.type != 'CPU' for d in prefs.devices) else 'CPU'
sc.cycles.samples = SAMPLES; sc.cycles.use_denoising = True
print('MAPSMITH device', prefs.compute_device_type, sc.cycles.device, flush=True)
sc.view_settings.view_transform = 'Standard'; sc.view_settings.exposure = -0.4

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
    tile = nt.nodes.new('ShaderNodeMapping'); tile.inputs['Scale'].default_value = (W / tm, D / tm, 1)
    nt.links.new(uv.outputs['UV'], tile.inputs[0])
    t = nt.nodes.new('ShaderNodeTexImage'); t.image = bpy.data.images.load(TEXFILES.get(str(i)) or f'{TEX}/{name}/diff_2k.jpg'); nt.links.new(tile.outputs[0], t.inputs[0])
    # relief: the layer's bump / displacement map, else flat
    hp = next((f for f in (f'{TEX}/{name}/bump.jpg', f'{TEX}/{name}/disp_2k.jpg') if os.path.exists(f)), None)
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

# ---------- models: one full copy per kind, linked duplicates for every placement ----------
lib = bpy.data.collections.new('lib'); sc.collection.children.link(lib)
lay = sc.view_layers[0].layer_collection.children['lib']; lay.exclude = True
proto = {}
CANVAS = json.load(open(os.environ['MAPSMITH_CANVAS'])) if os.environ.get('MAPSMITH_CANVAS') else {}
insts = []   # (item index, object) for the id pass
decals = []  # BG trees' painted shadows: not pieces, left out of the coverage and id passes
SIN, COS = math.sin(ELEV), math.cos(ELEV)
_day = max(0, math.sin((HOUR - 6) / 12 * math.pi)); _night = HOUR < 6.5 or HOUR > 20.5
SPRITE_K = 2 ** 0.4 * (0.3 if _night else min(1.0, 0.6 + _day * 0.6))   # undo the scene's -0.4 exposure; dim by night
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
                em = n.nodes.new('ShaderNodeEmission'); em.inputs['Strength'].default_value = SPRITE_K
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
for idx, it in enumerate(L['items']):
    mid = it['m']
    if idx % 10 == 0: print(f"MAPSMITH items {idx}/{len(L['items'])}", flush=True)
    if mid not in MODELS: continue
    if 'bgsprite' in MODELS[mid]['src']:
        tree_o, shad_o = bgsprite(mid, MODELS[mid]['src']['bgsprite'], it.get('s', 1))
        tree_o.location = shad_o.location = three_to_bl(it['p'])
        recolour(tree_o, it.get('col')); insts.append((idx, tree_o)); decals.append(shad_o); continue
    if mid not in proto:
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

# ---------- light: the editor's sun for that hour ----------
h = HOUR; day = max(0, math.sin((h - 6) / 12 * math.pi))
# BG's own light (measured on BG1's trees): south-west at 45°, all day; the hour sets colour and strength only
_hz = Vector((-0.61, 0, 0.79)).normalized() * math.cos(math.pi / 4)
d3 = Vector((_hz.x, math.sin(math.pi / 4), _hz.z))   # towards the sun, editor axes
dB = three_to_bl(d3)
night = h < 6.5 or h > 20.5
bpy.ops.object.light_add(type='SUN'); sun = bpy.context.object
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
    bpy.ops.object.light_add(type='POINT'); l = bpy.context.object
    # at least 0.7 m up: a light sitting in the pit burns its stones white
    l.location = three_to_bl(it['p']) + Vector((0, 0, max(0.7, f[0] * dims[2] * s_)))
    l.data.energy = f[1] * s_ * DAYK; l.data.color = (1.0, 0.52, 0.2); l.data.shadow_soft_size = f[2] * s_; nf += 1
print('fire lights', nf, flush=True)
w = bpy.data.worlds.new('w'); sc.world = w; w.use_nodes = True
w.node_tree.nodes['Background'].inputs[0].default_value = (0.04, 0.05, 0.1, 1) if night else (0.5, 0.56, 0.68, 1)
w.node_tree.nodes['Background'].inputs[1].default_value = 0.35 if night else 0.9

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
sc.render.image_settings.file_format = 'PNG'; sc.render.filepath = OUT
print('MAPSMITH stage render', flush=True)
bpy.ops.render.render(write_still=True)
print('rendered', OUT, flush=True)

if os.environ.get('BG3D_IDPASS'):
    # 1) coverage: the pieces alone (ground hidden), transparent background; the alpha keeps leaf cards' real holes
    for d_ in decals: d_.hide_render = True
    gnd.hide_render = True; sc.render.film_transparent = True; sc.cycles.samples = 16; sc.cycles.use_denoising = False
    sc.render.image_settings.color_mode = 'RGBA'
    print('MAPSMITH stage alpha', flush=True)
    sc.render.filepath = OUT[:-4] + '_alpha.png'; bpy.ops.render.render(write_still=True)
    gnd.hide_render = False; sc.render.film_transparent = False
    # 2) id pass: every piece in a flat colour that encodes its item index + 1, the ground black; no antialiasing, so
    # the game build can cut each piece's silhouette (id AND coverage) into a wall polygon
    sc.render.engine = 'BLENDER_WORKBENCH'
    sc.display.shading.light = 'FLAT'; sc.display.shading.color_type = 'OBJECT'
    sc.display.render_aa = 'OFF'; sc.view_settings.view_transform = 'Raw'; sc.view_settings.exposure = 0   # Raw: the colours stay exact numbers
    sc.display.shading.show_shadows = False; sc.display.shading.show_cavity = False
    gnd.color = (0, 0, 0, 1)
    for idx, inst in insts:
        v = idx + 1; inst.color = (((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255, 1)
    sc.render.image_settings.color_depth = '8'; sc.render.image_settings.color_mode = 'RGB'
    print('MAPSMITH stage id', flush=True)
    sc.render.filepath = OUT[:-4] + '_id.png'; bpy.ops.render.render(write_still=True)
    print('rendered id', sc.render.filepath, flush=True)
