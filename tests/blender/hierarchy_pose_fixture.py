"""Hierarchy and pairwise shape-key inputs for numerical and real viewer checks."""

import sys
from pathlib import Path

import bpy
from mathutils import Matrix

sys.path.insert(0, str(Path(__file__).parent))
import fixture


def create_scene():
    fixture.create_scene()
    scene = bpy.context.scene
    parent = fixture.identify(bpy.data.objects.new("parent", None), "fixture-parent")
    scene.collection.objects.link(parent)
    parent.hide_render = True  # Required transform ancestors remain in the export.
    parent.location = (0.2, 0, 0)
    parent.rotation_euler.z = 0.2
    parent.scale = (1.1, 0.9, 1.2)
    rig = bpy.data.objects["fixture-rig"]
    rig.parent = parent
    rig.hide_render = True
    bpy.data.objects["fixture-beam"].parent = parent
    cube = bpy.data.objects["fixture-cube"]
    cube.animation_data_clear()
    cube.parent = parent
    cube.matrix_parent_inverse = Matrix.Translation((-0.2, 0, 0))
    cube.rotation_euler.z = 0.3  # Composed world matrix has shear, local TRS does not.
    cube.data.materials.append(
        fixture.material("Second cube material", (0.1, 0.9, 0.3))
    )
    cube.data.polygons[0].material_index = 1
    panel = bpy.data.objects["fixture-panel"]
    panel.parent = parent
    panel.shape_key_add(name="Basis")
    key = panel.shape_key_add(name="Lean")
    for point in key.data:
        if point.co.z > 1:
            point.co.x += 0.8
            point.co.z += 0.3
    key.value = 0
    key.keyframe_insert("value", frame=1)
    key.value = 1
    key.keyframe_insert("value", frame=25)
    parent.keyframe_insert("location", index=0, frame=1)
    parent.location.x = 0.8
    parent.keyframe_insert("location", index=0, frame=25)
    scene.frame_set(1)
    bpy.context.view_layer.update()


def apply_command(command):
    action = command["action"]
    parent = bpy.data.objects.get("fixture-parent")
    panel = bpy.data.objects["fixture-panel"]
    cube = bpy.data.objects["fixture-cube"]
    if action == "frame":
        bpy.context.scene.frame_set(command["frame"])
    elif action == "bake_shape":
        # Independent reference: Blender evaluates the selected shape, then becomes
        # an ordinary static mesh through its own evaluated-mesh API.
        graph = bpy.context.evaluated_depsgraph_get()
        evaluated = panel.evaluated_get(graph)
        mesh = bpy.data.meshes.new_from_object(evaluated, depsgraph=graph)
        panel.data = mesh
    elif action == "reparent":
        cube.parent = parent if command.get("to_parent") else panel
        cube.matrix_parent_inverse = Matrix.Identity(4)
    elif action == "reverse_parent":
        # One full desired revision reverses an existing edge.
        panel.parent = None
        parent.parent = panel
    elif action == "remove_parent":
        bpy.data.objects.remove(parent, do_unlink=True)
    elif action == "shape_edit":
        panel.data.shape_keys.key_blocks[1].data[2].co.x += 0.3
    elif action == "shape_basis_edit":
        panel.data.shape_keys.key_blocks[0].data[0].co.x -= 0.2
    elif action == "shape_remove":
        panel.shape_key_clear()
    else:
        fixture.apply_command(command)
    bpy.context.view_layer.update()
