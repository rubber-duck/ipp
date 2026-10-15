"""Select the baked runtime scene, then use IPP's standard scene exporter.

blender -b cockpit.blend --python export_cockpit.py -- OUTPUT_DIRECTORY
"""

import runpy
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[5]
scene = bpy.data.scenes.get("Cockpit runtime")
if scene is None:
    raise RuntimeError("Run authoring/bake_cockpit.py before exporting the cockpit")
bpy.context.window.scene = scene
# Blender names are global across scenes; normalize only this unsaved export.
for obj in scene.objects:
    desired = obj.get("ipp_id")
    if not desired:
        continue
    conflict = bpy.data.objects.get(desired)
    if conflict is not None and conflict != obj:
        conflict.name = desired + "-authoring"
    obj.name = desired
runpy.run_path(str(ROOT / "integrations/blender/export_scene.py"), run_name="__main__")
