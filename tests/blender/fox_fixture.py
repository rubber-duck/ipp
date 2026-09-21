"""Reproducible Fox vertex animation and test-only independent baking commands."""

import math

import bpy


def add_vertex_animation():
    fox = bpy.data.objects["fox"]
    if fox.data.shape_keys is None:
        fox.shape_key_add(name="Basis")
        fox.shape_key_add(name="Chest puff")
    keys = fox.data.shape_keys
    if [key.name for key in keys.key_blocks] != ["Basis", "Chest puff"]:
        raise ValueError("Unexpected Fox shape keys; refusing to replace them")
    basis, puff = keys.key_blocks
    # Exaggerated breathing makes simultaneous deformation easy to see. Evaluate
    # in undeformed mesh space; the existing armature subsequently moves it.
    for original, target in zip(basis.data, puff.data, strict=True):
        x, y, z = original.co
        influence = math.exp(-(((y - 5) / 32) ** 2) - ((z - 48) / 20) ** 2)
        target.co = (x * (1 + 0.8 * influence), y, z + 12 * influence)
    if keys.animation_data and keys.animation_data.action:
        if keys.animation_data.action.name != "FoxBreathing":
            raise ValueError("Unexpected Fox shape action; refusing to replace it")
    for frame, weight in ((0, 0), (24, 1), (48, 0)):
        puff.value = weight
        puff.keyframe_insert("value", frame=frame)
    keys.animation_data.action.name = "FoxBreathing"
    bpy.context.scene.frame_set(0)
    bpy.context.view_layer.update()


def apply_command(command):
    if command["action"] == "frame":
        bpy.context.scene.frame_set(command["frame"])
    elif command["action"] == "unlit_comparison":
        for surface in bpy.data.objects["fox"].data.materials:
            surface["ipp_unlit"] = True
    elif command["action"] == "bake_current":
        fox = bpy.data.objects["fox"]
        graph = bpy.context.evaluated_depsgraph_get()
        mesh = bpy.data.meshes.new_from_object(
            fox.evaluated_get(graph), depsgraph=graph
        )
        fox.modifiers.clear()
        fox.data = mesh
    else:
        raise ValueError(f"Unknown Fox fixture action: {command['action']}")
    bpy.context.view_layer.update()


if __name__ == "__main__":
    add_vertex_animation()
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=bpy.data.filepath, compress=True)
