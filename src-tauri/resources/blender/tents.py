# Cloth-simulated tents: canvas meshes pinned on poles and stakes, gravity makes the sag and folds.
# build(S, objs) reads S['tent'] = {"kind": one of KINDS, "pattern_png": path | None, ...size options}.
# Every canvas mesh carries UVs with one convention, so a flat pattern image (tools/canvas.py, and the same generator
# in the map editor) dresses any tent the same way: u runs around / along the tent, v runs from the ground (0) up to the
# eaves (0.5) and on to the ridge or apex (1). The canvas material is named "canvas" so the editor can find it.
import bpy, math, random
from mathutils import Vector

TEX = 'E:/AI/bg3d/tex'


def canvas_mat(T):
    # the pattern image (colours, motif, wear, fabric grain) on the canvas UVs; a white one for the editor's light copy
    m = bpy.data.materials.new('canvas'); m.use_nodes = True; nt = m.node_tree; p = nt.nodes['Principled BSDF']
    t = nt.nodes.new('ShaderNodeTexImage')
    path = T.get('pattern_png')
    if path:
        t.image = bpy.data.images.load(path)
    else:
        t.image = bpy.data.images.new('white', 8, 8); t.image.pixels = [1.0] * (8 * 8 * 4)
    uv = nt.nodes.new('ShaderNodeUVMap'); uv.uv_map = 'UVMap'; nt.links.new(uv.outputs[0], t.inputs[0])
    nt.links.new(t.outputs['Color'], p.inputs['Base Color'])
    p.inputs['Roughness'].default_value = 0.92
    if path:   # the weave's relief, only in the full render
        tc = nt.nodes.new('ShaderNodeTexCoord'); mp = nt.nodes.new('ShaderNodeMapping'); mp.inputs['Scale'].default_value = (2.5,) * 3
        nt.links.new(tc.outputs['Object'], mp.inputs[0])
        n = nt.nodes.new('ShaderNodeTexImage'); n.image = bpy.data.images.load(f'{TEX}/rough_linen_nor_gl_2k.jpg')
        n.image.colorspace_settings.name = 'Non-Color'; n.projection = 'BOX'; n.projection_blend = 0.3
        nt.links.new(mp.outputs[0], n.inputs[0])
        nm = nt.nodes.new('ShaderNodeNormalMap'); nm.inputs['Strength'].default_value = 0.6
        nt.links.new(n.outputs[0], nm.inputs['Color']); nt.links.new(nm.outputs[0], p.inputs['Normal'])
    m.use_backface_culling = False
    return m

def pole_mat():
    m = bpy.data.materials.new('pole'); m.use_nodes = True
    m.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (0.18, 0.11, 0.06, 1)
    m.node_tree.nodes['Principled BSDF'].inputs['Roughness'].default_value = 0.8
    return m

def rope_mat():
    m = bpy.data.materials.new('rope'); m.use_nodes = True
    m.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (0.42, 0.34, 0.22, 1)
    return m

def pole(a, b, r, mat, objs):
    a, b = Vector(a), Vector(b); d = b - a
    bpy.ops.mesh.primitive_cylinder_add(radius=r, depth=d.length, vertices=8, location=(a + b) / 2)
    o = bpy.context.object; o.rotation_euler = d.to_track_quat('Z', 'Y').to_euler(); o.data.materials.append(mat); objs.append(o)

def simulate(obj, pins, frames, T):
    # pins: vertex indices held in place; everything else falls and folds
    vg = obj.vertex_groups.new(name='pin'); vg.add(pins, 1.0, 'REPLACE')
    cl = obj.modifiers.new('cloth', 'CLOTH'); s = cl.settings
    s.vertex_group_mass = 'pin'; s.quality = 8; s.mass = T.get('mass', 0.4)
    s.tension_stiffness = s.compression_stiffness = T.get('stiff', 5); s.bending_stiffness = T.get('bend', 0.5)
    cl.collision_settings.use_self_collision = False
    sc = bpy.context.scene; sc.frame_start = 1; sc.frame_end = frames
    cl.point_cache.frame_end = frames
    for f in range(1, frames + 1): sc.frame_set(f)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier='cloth')
    sm = obj.modifiers.new('smooth', 'SUBSURF'); sm.levels = sm.render_levels = 1
    sc.frame_set(1)

def grid_mesh(name, verts, faces, uvs):
    # uvs: one (u, v) per vertex; seams (u wrapping 1 -> 0) are fixed per face below
    me = bpy.data.meshes.new(name); me.from_pydata(verts, [], faces); me.update()
    layer = me.uv_layers.new(name='UVMap')
    for poly in me.polygons:
        us = [uvs[me.loops[li].vertex_index][0] for li in poly.loop_indices]
        wrap = max(us) - min(us) > 0.5
        for li in poly.loop_indices:
            u, v = uvs[me.loops[li].vertex_index]
            layer.data[li].uv = (u + 1 if wrap and u < 0.5 else u, v)
    o = bpy.data.objects.new(name, me); bpy.context.scene.collection.objects.link(o); return o

def ring_grid(seg, rings, at):
    # rows of `seg` vertices closed in a loop; at(j, i) -> (x, y, z, u, v)
    verts, uvs, faces = [], [], []
    for j in range(rings + 1):
        for i in range(seg):
            x, y, z, u, v = at(j, i); verts.append((x, y, z)); uvs.append((u, v))
    for j in range(rings):
        for i in range(seg): a, b = j * seg + i, j * seg + (i + 1) % seg; faces.append((a, b, b + seg, a + seg))
    return verts, faces, uvs

def flat_grid(nx, ny, at):
    # an open sheet; at(j, i) -> (x, y, z, u, v)
    verts, uvs = [], []
    for j in range(ny + 1):
        for i in range(nx + 1):
            x, y, z, u, v = at(j, i); verts.append((x, y, z)); uvs.append((u, v))
    faces = [(j * (nx + 1) + i, j * (nx + 1) + i + 1, (j + 1) * (nx + 1) + i + 1, (j + 1) * (nx + 1) + i) for j in range(ny) for i in range(nx)]
    return verts, faces, uvs

def drop_faces(verts, faces, uvs, keep):
    # remove faces (door gaps) and the loose vertices they leave, which would fall forever in the simulation
    faces = [f for f in faces if keep(f)]
    used = sorted({v for f in faces for v in f}); remap = {v: k for k, v in enumerate(used)}
    return [verts[v] for v in used], [tuple(remap[v] for v in f) for f in faces], [uvs[v] for v in used], remap

def round_tent(T, objs, mat, pm, sx=1.0):
    # round (or, with sx > 1, oval) pavilion: centre pole(s), cone roof sagging between rafters, wall on a ring of poles,
    # optional scalloped valance under the eaves
    R, hw, ha, n = T.get('radius', 1.6), T.get('wall', 1.3), T.get('apex', 2.9), T.get('poles', 8)
    seg, rings = n * 6, 14
    def roof_at(j, i):
        t = j / rings; a = 2 * math.pi * i / seg; r = R * 1.08 * t
        return r * math.cos(a) * sx, r * math.sin(a), ha - (ha - hw + 0.08) * t, i / seg, 1 - 0.5 * t
    v, f, uv = ring_grid(seg, rings, roof_at)
    roof = grid_mesh('roof', v, f, uv)
    pins = list(range(seg)) + [rings * seg + i * 6 for i in range(n)] + [j * seg + i * 6 for j in range(2, rings, 2) for i in range(n)]
    simulate(roof, pins, T.get('frames', 40), T); roof.data.materials.append(mat); objs.append(roof)
    wr = 8
    def wall_at(j, i):
        a = 2 * math.pi * i / seg; z = hw * (1 - j / wr)
        return R * math.cos(a) * sx, R * math.sin(a), z, i / seg, 0.5 * z / hw
    v, f, uv = ring_grid(seg, wr, wall_at)
    door = lambda face: not (T.get('door', True) and all(abs((fi % seg + 0.5) / seg - 0.75) < 0.06 for fi in face))
    v, f, uv, remap = drop_faces(v, f, uv, door)
    wall = grid_mesh('wall', v, f, uv)
    wpins = [remap[p] for p in [i * 6 for i in range(n)] + [wr * seg + i for i in range(0, seg, 3)] if p in remap]
    simulate(wall, wpins, T.get('frames', 40), {**T, 'mass': 0.3}); wall.data.materials.append(mat); objs.append(wall)
    if T.get('valance'):
        # a stiff band hanging from the eaves, its lower edge cut in scallops
        k, deep = T.get('scallops', 2), 0.32
        def val_at(j, i):
            a = 2 * math.pi * i / seg; r = R * 1.1
            z = hw + 0.06 if j == 0 else hw + 0.06 - deep * (0.55 + 0.45 * abs(math.sin(math.pi * (i % (seg // (n * k)) ) / (seg / (n * k)))))
            return r * math.cos(a) * sx, r * math.sin(a), z, i / seg, 0.5 if j == 0 else 0.42
        v, f, uv = ring_grid(seg, 1, val_at)
        val = grid_mesh('valance', v, f, uv); val.data.materials.append(mat); objs.append(val)
    tops = [(0, 0)] if sx == 1 else [(-R * sx * 0.35, 0), (R * sx * 0.35, 0)]
    for x, y in tops:
        pole((x, y, 0), (x, y, ha + 0.25), 0.045, pm, objs)
        bpy.ops.mesh.primitive_uv_sphere_add(radius=0.08, location=(x, y, ha + 0.28)); kb = bpy.context.object; kb.data.materials.append(pm); objs.append(kb)

def ridge_tent(T, objs, mat, pm):
    # ridge tent (a-frame, or the small soldier's tent): one sheet over a ridge pole, staked along both edges
    L, W, H = T.get('length', 2.4), T.get('width', 2.0), T.get('height', 1.6)
    nx, ny = 24, 20
    def at(j, i):
        s = j / ny * 2 - 1
        return -L / 2 + L * i / nx, s * W / 2, H * (1 - abs(s)) + 0.02, i / nx, 1 - abs(s)
    v, f, uv = flat_grid(nx, ny, at)
    sheet = grid_mesh('ridge', v, f, uv)
    ridge = [(ny // 2) * (nx + 1) + i for i in (0, nx)]
    stakes = [j * (nx + 1) + i for j in (0, ny) for i in range(0, nx + 1, 8)]
    simulate(sheet, ridge + stakes, T.get('frames', 40), T); sheet.data.materials.append(mat); objs.append(sheet)
    for x in (-L / 2, L / 2): pole((x, 0, 0), (x, 0, H + 0.15), 0.035, pm, objs)
    pole((-L / 2 - 0.1, 0, H + 0.04), (L / 2 + 0.1, 0, H + 0.04), 0.03, pm, objs)

def tarp(T, objs, mat, pm):
    # lean-to: sheet tied to two tall poles at the back, staked low at the front
    L, D, H = T.get('length', 2.6), T.get('depth', 2.0), T.get('height', 1.8)
    nx, ny = 26, 20
    v, f, uv = flat_grid(nx, ny, lambda j, i: (-L / 2 + L * i / nx, D / 2 - D * j / ny, H - (H - 0.15) * j / ny, i / nx, 1 - j / ny))
    sheet = grid_mesh('tarp', v, f, uv)
    simulate(sheet, [0, nx, nx // 2] + [ny * (nx + 1) + i for i in range(0, nx + 1, 5)], T.get('frames', 40), {**T, 'mass': 0.5})
    sheet.data.materials.append(mat); objs.append(sheet)
    for x in (-L / 2, L / 2): pole((x, D / 2, 0), (x, D / 2, H + 0.1), 0.035, pm, objs)

def command(T, objs, mat, pm):
    # large wall tent: four straight walls on a pole frame, a gable roof with overhang, the front door flaps tied open
    L, Wd, hw, H = T.get('length', 4.4), T.get('width', 3.2), T.get('wall', 1.6), T.get('height', 2.9)
    per = 2 * (L + Wd); nseg, wr = 64, 8
    def perim(t):
        # a point at fraction t of the walls' perimeter, starting at the front-left corner, going round anticlockwise
        d = t * per
        if d < L: return -L / 2 + d, -Wd / 2
        d -= L
        if d < Wd: return L / 2, -Wd / 2 + d
        d -= Wd
        if d < L: return L / 2 - d, Wd / 2
        return -L / 2, Wd / 2 - (d - L)
    def wall_at(j, i):
        x, y = perim(i / nseg); z = hw * (1 - j / wr); return x, y, z, i / nseg, 0.5 * z / hw
    v, f, uv = ring_grid(nseg, wr, wall_at)
    door_c = L / 2 / per   # the middle of the front wall (facing the camera)
    v, f, uv, remap = drop_faces(v, f, uv, lambda face: not all(abs((fi % nseg + 0.5) / nseg - door_c) < 0.045 for fi in face))
    corners = [round(c * nseg) % nseg for c in (0, L / per, (L + Wd) / per, (2 * L + Wd) / per)]
    pins = [remap[p] for p in [c for c in corners] + [i for i in range(0, nseg, 4)] + [wr * nseg + i for i in range(0, nseg, 4)] if p in remap]
    wall = grid_mesh('wall', v, f, uv); simulate(wall, pins, T.get('frames', 40), {**T, 'mass': 0.3})
    wall.data.materials.append(mat); objs.append(wall)
    nx, ny = 28, 20; ov = 0.25
    def roof_at(j, i):
        s = j / ny * 2 - 1
        return -L / 2 - ov + (L + 2 * ov) * i / nx, s * (Wd / 2 + ov), H - (H - hw + 0.12) * abs(s), i / nx, 1 - 0.5 * abs(s)
    v, f, uv = flat_grid(nx, ny, roof_at)
    roof = grid_mesh('roof', v, f, uv)
    rp = [(ny // 2) * (nx + 1) + i for i in range(0, nx + 1, 4)] + [j * (nx + 1) + i for j in (0, ny) for i in range(0, nx + 1, 4)]
    simulate(roof, rp, T.get('frames', 40), T); roof.data.materials.append(mat); objs.append(roof)
    for x in (-L / 2, 0, L / 2): pole((x, 0, 0), (x, 0, H + 0.12), 0.05, pm, objs)
    pole((-L / 2 - 0.15, 0, H + 0.05), (L / 2 + 0.15, 0, H + 0.05), 0.04, pm, objs)
    for x in (-L / 2, L / 2):
        for y in (-Wd / 2, Wd / 2): pole((x, y, 0), (x, y, hw + 0.05), 0.035, pm, objs)
    # the two door flaps, rolled up beside the opening
    half = 0.045 * per   # the door's half width, as cut out of the front wall above
    for side in (-1, 1):
        bpy.ops.mesh.primitive_cylinder_add(radius=0.09, depth=hw * 0.9, vertices=10, location=(side * (half + 0.12), -Wd / 2 - 0.05, hw * 0.5))
        c = bpy.context.object; c.data.materials.append(mat); objs.append(c)

def canopy(T, objs, mat, pm):
    # open-sided awning: a square sheet on four corner posts and a taller centre post, a short valance all round
    L, D, hp, hc = T.get('length', 3.4), T.get('depth', 3.0), T.get('posts', 2.1), T.get('peak', 2.7)
    nx, ny = 24, 22
    v, f, uv = flat_grid(nx, ny, lambda j, i: (-L / 2 + L * i / nx, -D / 2 + D * j / ny, hp + 0.1, i / nx, 0.5 + 0.5 * j / ny))
    sheet = grid_mesh('canopy', v, f, uv)
    c = (ny // 2) * (nx + 1) + nx // 2
    for k in range(len(sheet.data.vertices)):   # start as a low pyramid so the peak pin does not tear the sheet
        p = sheet.data.vertices[k].co; t = 1 - max(abs(p.x) / (L / 2), abs(p.y) / (D / 2)); p.z = hp + (hc - hp) * t
    pins = [0, nx, ny * (nx + 1), ny * (nx + 1) + nx, c]
    simulate(sheet, pins, T.get('frames', 40), {**T, 'mass': 0.35}); sheet.data.materials.append(mat); objs.append(sheet)
    for x in (-L / 2, L / 2):
        for y in (-D / 2, D / 2): pole((x, y, 0), (x, y, hp + 0.15), 0.04, pm, objs)
    pole((0, 0, 0), (0, 0, hc + 0.05), 0.045, pm, objs)

def teepee(T, objs, mat, pm):
    # tipi: a steep cone wrapped round crossed poles that stick out above the smoke hole, door at the front
    R, H, n = T.get('radius', 1.7), T.get('height', 3.6), T.get('poles', 9)
    seg, rings = n * 6, 16; top = 0.18
    def at(j, i):
        t = j / rings; a = 2 * math.pi * i / seg; r = R * (top / R + (1 - top / R) * t)
        return r * math.cos(a), r * math.sin(a), H * (1 - t) * 0.97, i / seg, 1 - t
    v, f, uv = ring_grid(seg, rings, at)
    v, f, uv, remap = drop_faces(v, f, uv, lambda face: not all(abs((fi % seg + 0.5) / seg - 0.75) < 0.05 and fi // seg > rings * 0.6 for fi in face))
    cone = grid_mesh('tipi', v, f, uv)
    pins = [remap[p] for p in list(range(seg)) + [rings * seg + i for i in range(0, seg, 3)] + [j * seg + i * 6 for j in range(rings + 1) for i in range(n)] if p in remap]
    simulate(cone, pins, T.get('frames', 30), {**T, 'mass': 0.3}); cone.data.materials.append(mat); objs.append(cone)
    for i in range(n):
        a = 2 * math.pi * i / n + 0.2
        pole((R * 1.02 * math.cos(a), R * 1.02 * math.sin(a), 0), (-0.35 * math.cos(a), -0.35 * math.sin(a), H + 0.7), 0.035, pm, objs)

KINDS = {'pavilion': lambda T, o, m, p: round_tent(T, o, m, p), 'oval': lambda T, o, m, p: round_tent(T, o, m, p, T.get('stretch', 1.7)),
         'aframe': ridge_tent, 'tarp': tarp, 'command': command, 'canopy': canopy, 'teepee': teepee}

def build(S, objs):
    T = S['tent']; random.seed(T.get('seed', 1))
    mat, pm = canvas_mat(T), pole_mat()
    n0 = len(objs)
    KINDS[T.get('kind', 'pavilion')](T, objs, mat, pm)
    if T.get('rot'):
        from mathutils import Matrix
        R = Matrix.Rotation(math.radians(T['rot']), 4, 'Z')
        for o in objs[n0:]: o.matrix_world = R @ o.matrix_world
