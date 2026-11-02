# Where the render scripts find things, set by MapSmithEE (src-tauri/src/camprender) through the environment:
#   MAPSMITH_LIBRARY  the model library (bg3d layout: models.json, assets/, mesh/, proxies/, tex/, ext/)
#   MAPSMITH_WORK     a folder for generated files (ground masks)
# Paths stored in models.json may be relative to the library, or absolute ones from the machine the library was made on
# (E:/AI/bg3d/..., E:/AI/objaverse/glb/...): those are looked up in the library by their tail when they do not exist.
import os

def _dir(var):
    v = os.environ.get(var, '').replace('\\', '/').rstrip('/')
    if not v: raise RuntimeError(f'{var} is not set')
    return v

LIB = _dir('MAPSMITH_LIBRARY')
WORK = _dir('MAPSMITH_WORK')
TEX = f'{LIB}/tex'
ASSETS = f'{LIB}/assets'

def lib(p):
    """a library file path from models.json, resolved on this machine"""
    p = p.replace('\\', '/')
    if not os.path.isabs(p): return f'{LIB}/{p}'
    if os.path.exists(p): return p
    i = p.lower().find('/bg3d/')
    if i >= 0 and os.path.exists(f'{LIB}/{p[i + 6:]}'): return f'{LIB}/{p[i + 6:]}'
    tail = '/'.join(p.split('/')[-2:])   # e.g. glb/<id>.glb
    if os.path.exists(f'{LIB}/{tail}'): return f'{LIB}/{tail}'
    return p
