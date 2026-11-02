# One model, loaded the way the editor sees it: every part joined into one mesh, fitted to its real-world size,
# centred on x/y and standing on z = 0. Shared by proxy.py (light editor copy) and render_map.py (full render).
import bpy, os, sys
from mathutils import Vector
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from paths import ASSETS, lib

def load(M, name='model'):
    sc = bpy.context.scene
    before = set(bpy.data.objects)
    src = M['src']
    if 'tent' in src:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import tents
        tents.build({'tent': src['tent']}, [])
    elif 'logseat' in src:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import logseat
        logseat.build(src['logseat'])
    else:
        path = lib(src['glb']) if 'glb' in src else [f"{ASSETS}/{src['asset']}/{f}" for f in os.listdir(f"{ASSETS}/{src['asset']}") if f.endswith('.gltf')][0]
        bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    if src.get('only'):
        keep = {o for o in new if o.parent is None and o.name.startswith(src['only'])}
        for o in list(new):
            r = o
            while r.parent: r = r.parent
            if r not in keep: bpy.data.objects.remove(o, do_unlink=True)
        new = [o for o in bpy.data.objects if o not in before]
    meshes = [o for o in new if o.type == 'MESH']
    if M.get('rot'):   # a model made standing (a pelt hung on a wall): turned before joining, degrees about X, Y, Z
        import math
        from mathutils import Euler
        R = Euler([math.radians(v) for v in M['rot']], 'XYZ').to_matrix().to_4x4()
        for o in new:
            if o.parent is None: o.matrix_world = R @ o.matrix_world
    bpy.ops.object.select_all(action='DESELECT')
    for o in meshes:
        o.hide_set(False); o.select_set(True)
        bpy.context.view_layer.objects.active = o
        for m in list(o.modifiers):
            try: bpy.ops.object.modifier_apply(modifier=m.name)
            except RuntimeError: o.modifiers.remove(m)
    for o in meshes: o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM')
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    names = [o.name for o in new]   # joined objects are freed: keep names, not references
    if len(meshes) > 1: bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active; ob.name = name
    for n in names:
        o = bpy.data.objects.get(n)
        if o and o is not ob: bpy.data.objects.remove(o, do_unlink=True)
    vs = [v.co for v in ob.data.vertices]
    lo = Vector([min(v[i] for v in vs) for i in range(3)]); hi = Vector([max(v[i] for v in vs) for i in range(3)])
    ax, m = M.get('fit', ['max', 1.0])
    k = m / ((hi - lo).z if ax == 'z' else max(hi - lo)) if ax != 'none' else M.get('scale', 1.0)
    ob.location = (-(lo.x + hi.x) / 2 * k, -(lo.y + hi.y) / 2 * k, -lo.z * k); ob.scale = (k, k, k)
    bpy.ops.object.select_all(action='DESELECT'); ob.select_set(True); bpy.context.view_layer.objects.active = ob
    bpy.ops.object.transform_apply(location=True, scale=True)
    return ob
