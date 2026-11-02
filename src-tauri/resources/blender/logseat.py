# Rustic seats assembled from scanned Poly Haven trunks and stumps (CC0): a split log on two stump legs, a log lying on
# the ground with its top cut flat, a stump sawn to stool height, and the split-log bench with a backrest of branches.
# build(T) leaves the parts in the scene; norm.load joins and grounds them like any model.
# T = {"kind": "halflog"|"log"|"stool"|"backrest", "length": m, "height": m, "trunk": asset, "stump": asset, "seed": n}
import bpy, bmesh, math, os, random
from mathutils import Vector, Matrix

ASSETS = 'E:/AI/bg3d/assets'

def load(asset, only=None):
    before = set(bpy.data.objects)
    d = f'{ASSETS}/{asset}'
    bpy.ops.import_scene.gltf(filepath=[f'{d}/{f}' for f in os.listdir(d) if f.endswith('.gltf')][0])
    new = [o for o in bpy.data.objects if o not in before]
    if only:   # one variant of a multi-piece set
        for o in [o for o in new if o.parent is None and not o.name.startswith(only)]:
            for c in list(o.children_recursive): bpy.data.objects.remove(c, do_unlink=True)
            bpy.data.objects.remove(o, do_unlink=True)
        new = [o for o in bpy.data.objects if o not in before]
    ms = [o for o in new if o.type == 'MESH']
    for o in new:
        if o.type != 'MESH': bpy.data.objects.remove(o, do_unlink=True)
    bpy.ops.object.select_all(action='DESELECT')
    for o in ms: o.select_set(True)
    bpy.context.view_layer.objects.active = ms[0]
    bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM'); bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    if len(ms) > 1: bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.rotation_mode = 'XYZ'   # glTF imports use quaternions: the Euler turns below would be ignored
    return o

def bounds(o):
    vs = [o.matrix_world @ v.co for v in o.data.vertices]
    return Vector([min(v[i] for v in vs) for i in range(3)]), Vector([max(v[i] for v in vs) for i in range(3)])

def apply(o):
    bpy.ops.object.select_all(action='DESELECT'); o.select_set(True); bpy.context.view_layer.objects.active = o
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

def lay_along_x(o):
    # turn the trunk so its longest extent runs along X, centred on the origin
    lo, hi = bounds(o); size = hi - lo
    ax = max(range(3), key=lambda i: size[i])
    if ax == 1: o.rotation_euler = (0, 0, math.pi / 2)
    elif ax == 2: o.rotation_euler = (0, math.pi / 2, 0)
    apply(o); lo, hi = bounds(o); o.location = -(lo + hi) / 2; apply(o)

def sawn_mat():
    # fresh-sawn wood (Poly Haven oak_veneer_01, CC0) for every face the saw leaves
    m = bpy.data.materials.get('sawn')
    if m: return m
    m = bpy.data.materials.new('sawn'); m.use_nodes = True; n = m.node_tree; p = n.nodes['Principled BSDF']
    t = n.nodes.new('ShaderNodeTexImage'); t.image = bpy.data.images.load('E:/AI/bg3d/tex/oak_veneer_01/diff_2k.jpg')
    n.links.new(t.outputs['Color'], p.inputs['Base Color']); p.inputs['Roughness'].default_value = 0.85
    return m

def _hull(p):
    """2D convex hull (monotone chain), counter-clockwise"""
    p = sorted(set(p))
    if len(p) < 3: return p
    def cross(o, a, b): return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lo, hi = [], []
    for q in p:
        while len(lo) >= 2 and cross(lo[-2], lo[-1], q) <= 0: lo.pop()
        lo.append(q)
    for q in reversed(p):
        while len(hi) >= 2 and cross(hi[-2], hi[-1], q) <= 0: hi.pop()
        hi.append(q)
    return lo[:-1] + hi[:-1]

def cut(o, z, keep_below=True, axis=2):
    # saw the mesh flat at `z` along `axis` and close the cut with a face of fresh wood, its grain mapped flat on it
    if 'sawn' not in o.data.materials: o.data.materials.append(sawn_mat())
    mi = list(o.data.materials).index(bpy.data.materials['sawn'])
    bm = bmesh.new(); bm.from_mesh(o.data)
    uvl = bm.loops.layers.uv.active or bm.loops.layers.uv.new()
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    co, no = [0, 0, 0], [0, 0, 0]; co[axis] = z; no[axis] = 1
    res = bmesh.ops.bisect_plane(bm, geom=geom, plane_co=co, plane_no=no, clear_outer=keep_below, clear_inner=not keep_below)
    # close the cut with one cap: the outline (convex hull) of the cut's points, in fresh wood. Scanned trunks are not
    # watertight, so filling the cut's own edge loops left most cuts open (no seat top at all)
    a, b = [i for i in range(3) if i != axis]
    pts = {(round(v.co[a], 5), round(v.co[b], 5)) for v in bm.verts if abs(v.co[axis] - z) < 1e-4}
    hull = _hull(list(pts))
    if len(hull) >= 3:
        vs = []
        for pa, pb in hull:
            c = [0.0, 0.0, 0.0]; c[a], c[b], c[axis] = pa, pb, z; vs.append(bm.verts.new(c))
        f = bm.faces.new(vs); f.material_index = mi
        bmesh.ops.recalc_face_normals(bm, faces=[f])
        if (f.normal[axis] > 0) != keep_below: f.normal_flip()      # facing out of the kept wood
        for lp in f.loops: lp[uvl].uv = (lp.vert.co[a] / 0.6, lp.vert.co[b] / 0.6)   # 0.6 m of wood per image
    bm.to_mesh(o.data); bm.free(); o.data.update()

def fit_len(o, L):
    lo, hi = bounds(o); k = L / (hi.x - lo.x); o.scale = (k, k, k); apply(o)

def log_piece(asset, L, diam):
    # a straight stretch of the trunk: scaled so it is `diam` thick, then sawn to length L around its middle
    o = load(asset); lay_along_x(o)
    lo, hi = bounds(o); k = diam / max(hi.y - lo.y, hi.z - lo.z); o.scale = (k, k, k); apply(o)
    lo, hi = bounds(o); c = (lo.x + hi.x) / 2
    cut(o, c + L / 2, keep_below=True, axis=0); cut(o, c - L / 2, keep_below=False, axis=0)
    lo, hi = bounds(o); o.location = -(lo + hi) / 2; apply(o)
    return o

def stump_leg(asset, h, width):
    st = load(asset); lo, hi = bounds(st)
    st.location = (-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z); apply(st)
    lo, hi = bounds(st); cut(st, min(h, (hi.z - lo.z) * 0.9), keep_below=True)
    lo, hi = bounds(st); k = width / max(hi.x - lo.x, hi.y - lo.y); st.scale = (k, k, max(0.2, h / max(0.05, hi.z - lo.z)) if hi.z - lo.z < h else 1); apply(st)
    return st

def halflog(T, legs=True):
    L, H = T.get('length', 1.8), T.get('height', 0.45)
    log = log_piece(T.get('trunk', 'dead_tree_trunk'), L, T.get('diam', 0.42))
    cut(log, 0.0, keep_below=True)                       # split lengthwise: the flat face up
    lo, hi = bounds(log); log.location.z = H - hi.z; apply(log)
    parts = [log]
    leg_h = H - (hi.z - lo.z) + 0.04
    for x in (-L * 0.3, L * 0.3):
        st = stump_leg(T.get('stump', 'tree_stump_02'), leg_h, 0.42); st.location = (x, 0, 0); apply(st); parts.append(st)
    return parts

def log_seat(T):
    # a whole log lying on the ground, its top sawn flat to sit on
    L, d = T.get('length', 2.2), T.get('diam', 0.5)
    log = log_piece(T.get('trunk', 'dead_tree_trunk_02'), L, d)
    lo, hi = bounds(log); log.location.z = -lo.z; apply(log)
    lo, hi = bounds(log); cut(log, lo.z + (hi.z - lo.z) * 0.8, keep_below=True)
    return [log]

def stool(T):
    return [stump_leg(T.get('stump', 'tree_stump_02'), T.get('height', 0.45), T.get('width', 0.5))]

def backrest(T):
    # the split-log bench, with two thin posts behind the seat and a round rail lashed across their tops
    parts = halflog(T)
    L, H = T.get('length', 1.8), T.get('height', 0.45)
    for x in (-L * 0.38, L * 0.38):
        p = log_piece(T.get('trunk', 'dead_tree_trunk'), 0.95, 0.09)
        p.rotation_euler = (0, math.pi / 2 - 0.1, 0); apply(p)
        lo, hi = bounds(p); p.location = (x - (lo.x + hi.x) / 2, 0.2, -lo.z + 0.02); apply(p); parts.append(p)
    r = log_piece(T.get('trunk', 'dead_tree_trunk'), L * 0.95, 0.11)
    r.location = (0, 0.25, 0.9); apply(r); parts.append(r)
    return parts

def build(T):
    return {'halflog': halflog, 'log': log_seat, 'stool': stool, 'backrest': backrest}[T.get('kind', 'halflog')](T)
