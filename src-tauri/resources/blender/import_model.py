# "Import model": any file Blender reads becomes a user piece (MapSmithEE userlib).
# Usage: blender -b -P import_model.py -- <source file> <userlib dir> <id> <height m | ->
#   assets/<id>.glb      the full model as imported (textures embedded), what the render loads (norm.py fits it)
#   editor/<id>.glb      the editor's light copy: fitted, decimated, small colour textures (proxy.py's recipe)
#   thumbs/<id>.png      a 128 px BG-view thumbnail
# Prints 'MAPSMITH result {"dims": [x, y, z], "tris": n, "fit": [...]}' (dims in metres: width, depth, height).
import bpy, sys, os, json, math
from mathutils import Vector
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.dont_write_bytecode = True   # the scripts may sit in a read-only install folder

a = sys.argv[sys.argv.index('--') + 1:]
SRC, UL, ID = os.path.abspath(a[0]), os.path.abspath(a[1]).replace('\\', '/').rstrip('/'), a[2]
HEIGHT = float(a[3]) if len(a) > 3 and a[3] not in ('', '-') else None
os.environ.setdefault('MAPSMITH_LIBRARY', UL); os.environ.setdefault('MAPSMITH_WORK', UL)
import norm
for d in ('assets', 'editor', 'thumbs'): os.makedirs(f'{UL}/{d}', exist_ok=True)

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
ext = os.path.splitext(SRC)[1].lower()
IMPORT = {
    '.glb': lambda p: bpy.ops.import_scene.gltf(filepath=p), '.gltf': lambda p: bpy.ops.import_scene.gltf(filepath=p),
    '.obj': lambda p: bpy.ops.wm.obj_import(filepath=p), '.fbx': lambda p: bpy.ops.import_scene.fbx(filepath=p),
    '.stl': lambda p: bpy.ops.wm.stl_import(filepath=p), '.ply': lambda p: bpy.ops.wm.ply_import(filepath=p),
    '.dae': lambda p: bpy.ops.wm.collada_import(filepath=p),
    '.usd': lambda p: bpy.ops.wm.usd_import(filepath=p), '.usda': lambda p: bpy.ops.wm.usd_import(filepath=p),
    '.usdc': lambda p: bpy.ops.wm.usd_import(filepath=p), '.usdz': lambda p: bpy.ops.wm.usd_import(filepath=p),
    '.abc': lambda p: bpy.ops.wm.alembic_import(filepath=p), '.x3d': lambda p: bpy.ops.import_scene.x3d(filepath=p),
    '.blend': None,
}
if ext not in IMPORT: raise SystemExit(f'MAPSMITH error: {ext} files are not supported')
if ext == '.blend':
    with bpy.data.libraries.load(SRC, link=False) as (have, want): want.objects = list(have.objects)
    for o in want.objects:
        if o and o.type in ('MESH', 'EMPTY'): sc.collection.objects.link(o)
else:
    try: IMPORT[ext](SRC)
    except Exception as e: raise SystemExit(f'MAPSMITH error: Blender could not read the file ({e})')
# cameras, lights, armatures out; a mesh under one keeps where it was (an FBX rig carries the model's turn and scale)
bpy.context.view_layer.update()
for o in sc.objects:
    if o.parent and o.parent.type not in ('MESH', 'EMPTY'): mw = o.matrix_world.copy(); o.parent = None; o.matrix_world = mw
for o in [o for o in sc.objects if o.type not in ('MESH', 'EMPTY')]: bpy.data.objects.remove(o, do_unlink=True)
meshes = [o for o in sc.objects if o.type == 'MESH']
if not meshes: raise SystemExit('MAPSMITH error: the file has no mesh')

# the full copy, as glTF (what the library's models are): the render loads it like any other piece
full = f'{UL}/assets/{ID}.glb'
bpy.ops.export_scene.gltf(filepath=full, export_format='GLB', use_selection=False, export_apply=True, export_cameras=False, export_lights=False)

# its size: the height asked, else as made (metres); a model over 50 m was made in centimetres (a 1.2 m chair is 120 units)
lo = Vector([min((o.matrix_world @ Vector(c))[i] for o in meshes for c in o.bound_box) for i in range(3)])
hi = Vector([max((o.matrix_world @ Vector(c))[i] for o in meshes for c in o.bound_box) for i in range(3)])
size = max(hi - lo)
if HEIGHT: fit = ['z', HEIGHT]
elif size > 50: fit = ['none', 0.01]
else: fit = ['none', 1.0]
M = {'src': {'glb': full}, 'fit': fit if fit[0] != 'none' else ['none', 1], 'scale': fit[1] if fit[0] == 'none' else 1.0}

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
ob = norm.load(M, ID)
dims = [round(v, 3) for v in ob.dimensions]
tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)

# thumbnail: the BG view (38 degrees down, from the south), transparent background
e = math.radians(38)
bpy.ops.object.camera_add(); cam = bpy.context.object; sc.camera = cam; cam.data.type = 'ORTHO'
bpy.ops.object.light_add(type='SUN'); sun = bpy.context.object; sun.data.energy = 3.5
sun.rotation_euler = Vector((-0.5, -0.6, 0.7)).to_track_quat('Z', 'Y').to_euler()
w = bpy.data.worlds.new('w'); sc.world = w; w.use_nodes = True; w.node_tree.nodes['Background'].inputs[1].default_value = 0.8
engines = [i.identifier for i in bpy.types.RenderSettings.bl_rna.properties['engine'].enum_items]
sc.render.engine = 'BLENDER_EEVEE_NEXT' if 'BLENDER_EEVEE_NEXT' in engines else 'BLENDER_EEVEE'
sc.render.film_transparent = True; sc.view_settings.view_transform = 'Standard'
sc.render.image_settings.file_format = 'PNG'; sc.render.image_settings.color_mode = 'RGBA'
d = ob.dimensions
cam.rotation_euler = (math.pi / 2 - e, 0, 0)
cam.location = Vector((0, 0, d.z / 2)) + Vector((0, -40 * math.cos(e), 40 * math.sin(e)))
cam.data.ortho_scale = max(d.x, d.y * math.sin(e) + d.z * math.cos(e)) * 1.1
sc.render.resolution_x = sc.render.resolution_y = 128
sc.render.filepath = f'{UL}/thumbs/{ID}.png'; bpy.ops.render.render(write_still=True)

# the editor's copy: decimated, colour textures only, small
if tris > 6000:
    m = ob.modifiers.new('dec', 'DECIMATE'); m.ratio = max(0.02, 6000 / tris); m.use_collapse_triangulate = True
    bpy.context.view_layer.objects.active = ob; bpy.ops.object.modifier_apply(modifier='dec')
for mat in bpy.data.materials:
    if not mat.use_nodes: continue
    p = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
    if not p: continue
    for name in ('Normal', 'Roughness', 'Metallic', 'Specular IOR Level', 'Emission Color'):
        s = p.inputs.get(name)
        if s:
            for l in list(s.links): mat.node_tree.links.remove(l)
side = 256 if max(d) < 1.2 else 384
for img in bpy.data.images:
    if img.size[0] > side:
        f = side / max(img.size); img.scale(max(1, int(img.size[0] * f)), max(1, int(img.size[1] * f)))
for o in [o for o in sc.objects if o.type in ('CAMERA', 'LIGHT')]: bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.export_scene.gltf(filepath=f'{UL}/editor/{ID}.glb', export_format='GLB', export_image_format='JPEG', export_jpeg_quality=80,
                          use_selection=False, export_apply=True, export_cameras=False, export_lights=False)
print('MAPSMITH result ' + json.dumps({'dims': dims, 'tris': tris, 'fit': M['fit'], 'scale': M['scale']}), flush=True)
