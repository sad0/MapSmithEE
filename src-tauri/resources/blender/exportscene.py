# "Export scene": the scene render_map.py built (BG camera, the hour's sun and fires, painted ground, every piece),
# saved for another 3D program instead of rendered. MAPSMITH_EXPORT = {"path": ..., "format": "blend" | "glb" | "fbx"}.
#   blend  the scene as it is (procedural ground, Cycles settings), every image packed in: renders as MapSmithEE does
#   glb    glTF binary; fbx  FBX with its textures embedded. Both need plain materials: the painted ground is baked
#          into one picture, BG sprites become textured cut-outs, pieces lose their own colour (its node group)
import bpy, os


def _bake_ground(gnd, sc, out_dir, px):
    """the ground's painted colour baked into one picture (at about `px` per metre, 8192 at most)"""
    w_m, d_m = gnd.dimensions.x, gnd.dimensions.y
    k = min(px, 8192 / max(w_m, d_m))
    img = bpy.data.images.new('ground_baked', max(16, round(w_m * k)), max(16, round(d_m * k)))
    mat = gnd.data.materials[0]; nt = mat.node_tree
    node = nt.nodes.new('ShaderNodeTexImage'); node.image = img
    for n in nt.nodes: n.select = False
    node.select = True; nt.nodes.active = node
    bpy.ops.object.select_all(action='DESELECT'); gnd.select_set(True); bpy.context.view_layer.objects.active = gnd
    sc.render.engine = 'CYCLES'; sc.cycles.samples = 4
    sc.render.bake.use_pass_direct = False; sc.render.bake.use_pass_indirect = False; sc.render.bake.use_pass_color = True
    print('MAPSMITH stage bake', flush=True)
    bpy.ops.object.bake(type='DIFFUSE', pass_filter={'COLOR'})
    img.filepath_raw = os.path.join(out_dir, 'ground_baked.png').replace('\\', '/'); img.file_format = 'PNG'; img.save()
    # the ground keeps only that picture
    nt.nodes.clear()
    o = nt.nodes.new('ShaderNodeOutputMaterial'); p = nt.nodes.new('ShaderNodeBsdfPrincipled'); t = nt.nodes.new('ShaderNodeTexImage')
    t.image = img; p.inputs['Roughness'].default_value = 1.0
    nt.links.new(t.outputs['Color'], p.inputs['Base Color']); nt.links.new(p.outputs[0], o.inputs['Surface'])


def _plain_sprite(m):
    """a BG sprite's emission / transparency mix -> a Principled cut-out the exporters understand"""
    nt = m.node_tree
    tex = next((n for n in nt.nodes if n.type == 'TEX_IMAGE'), None)
    if not tex: return
    shadow = m.name.endswith('_shadow')
    img = tex.image; nt.nodes.clear()
    o = nt.nodes.new('ShaderNodeOutputMaterial'); p = nt.nodes.new('ShaderNodeBsdfPrincipled'); t = nt.nodes.new('ShaderNodeTexImage'); t.image = img
    if shadow:
        p.inputs['Base Color'].default_value = (0, 0, 0, 1)
    else:
        nt.links.new(t.outputs['Color'], p.inputs['Base Color']); nt.links.new(t.outputs['Color'], p.inputs['Emission Color'])
        p.inputs['Emission Strength'].default_value = 1.0
    nt.links.new(t.outputs['Alpha'], p.inputs['Alpha']); nt.links.new(p.outputs[0], o.inputs['Surface'])
    if hasattr(m, 'surface_render_method'): m.surface_render_method = 'BLENDED' if shadow else 'DITHERED'


def _drop_colour_groups(m):
    """reconnect whatever fed a piece-colour group straight to where the group went"""
    nt = m.node_tree
    for g in [n for n in nt.nodes if n.type == 'GROUP' and n.node_tree and n.node_tree.name.startswith('piece_colour')]:
        src = g.inputs[0].links[0].from_socket if g.inputs[0].is_linked else None
        dst = [l.to_socket for l in g.outputs[0].links]
        nt.nodes.remove(g)
        if src:
            for d in dst: nt.links.new(src, d)


def _imgs(m): return {n.image for n in m.node_tree.nodes if n.type == 'TEX_IMAGE'} if m and m.node_tree else set()


def _mesh_materials(sc):
    """glTF keeps materials on the mesh (a piece's object-linked copies split it): a recoloured copy showing the mesh
    material's pictures (its colour is dropped anyway) goes back to it; a piece whose pictures differ (a tent's
    canvas) gets its own mesh carrying them"""
    for o in sc.objects:
        if o.type != 'MESH' or not any(s.link == 'OBJECT' for s in o.material_slots): continue
        data = list(o.data.materials) + [None] * (len(o.material_slots) - len(o.data.materials))
        mats = [s.material if s.link == 'OBJECT' and s.material and _imgs(s.material) != _imgs(d) else d
                for s, d in zip(o.material_slots, data)]
        if any(m is not d for m, d in zip(mats, data)): o.data = o.data.copy()
        for i, s in enumerate(o.material_slots): s.link = 'DATA'; o.data.materials[i] = mats[i]


def save(E, sc, gnd, lib, sprites, pxm):
    path = E['path'].replace('\\', '/'); fmt = E.get('format', 'blend')
    out_dir = os.path.dirname(path); os.makedirs(out_dir, exist_ok=True)
    print('MAPSMITH stage export', flush=True)
    if fmt == 'blend':
        bpy.ops.file.pack_all()
        bpy.ops.wm.save_as_mainfile(filepath=path, compress=True, copy=True)
        print('exported', path, flush=True)
        return
    # the per-kind originals in the excluded 'lib' collection are not placed pieces: out (their copies share the mesh)
    for o in list(lib.all_objects): bpy.data.objects.remove(o, do_unlink=True)
    _bake_ground(gnd, sc, out_dir, pxm)
    _mesh_materials(sc)
    for m in bpy.data.materials:
        if not m.use_nodes: continue
        if any(m.name == f'{s}_tree' or m.name == f'{s}_shadow' for s in sprites): _plain_sprite(m)
        else: _drop_colour_groups(m)
    if fmt == 'glb':
        # the glTF exporter takes a sun's strength from its light nodes' Emission (1.0), not its energy
        for l in bpy.data.lights:
            em = l.type == 'SUN' and l.node_tree and next((n for n in l.node_tree.nodes if n.type == 'EMISSION'), None)
            if em: em.inputs['Strength'].default_value *= l.energy
        bpy.ops.export_scene.gltf(filepath=path, export_format='GLB', export_cameras=True, export_lights=True, export_apply=True)
    elif fmt == 'fbx':
        bpy.ops.export_scene.fbx(filepath=path, path_mode='COPY', embed_textures=True, object_types={'MESH', 'CAMERA', 'LIGHT', 'EMPTY'},
                                 apply_scale_options='FBX_SCALE_ALL', bake_space_transform=True)
    else:
        raise SystemExit(f'unknown export format {fmt}')
    print('exported', path, flush=True)
