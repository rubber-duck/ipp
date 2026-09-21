"""Shared deterministic Blender authoring helpers for the opt-in stress scene."""

import bpy
import numpy as np
from bpy_extras.anim_utils import action_ensure_channelbag_for_slot


def identify(obj, name):
    obj.name = name
    obj["ipp_id"] = name
    return obj


def material(name, color, alpha=1):
    value = bpy.data.materials.new(name)
    value.use_nodes = True
    shader = value.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = (*color, 1)
    shader.inputs["Roughness"].default_value = 0.65
    shader.inputs["Alpha"].default_value = alpha
    return value


def object_from_mesh(mesh, name, location, materials=None):
    obj = identify(bpy.data.objects.new(name, mesh), name)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    if materials:
        for index, value in enumerate(materials):
            obj.material_slots[index].link = "OBJECT"
            obj.material_slots[index].material = value
    return obj


def action_bag(obj, name):
    action = bpy.data.actions.new(name)
    data = obj.animation_data_create()
    data.action = action
    data.action_slot = action.slots.new(obj.id_type, obj.name)
    return action_ensure_channelbag_for_slot(action, data.action_slot)


def curve(bag, path, index, frames, values):
    points = bag.fcurves.new(path, index=index).keyframe_points
    points.add(len(frames))
    pairs = np.empty((len(frames), 2), dtype=np.float32)
    pairs[:, 0] = frames
    pairs[:, 1] = values
    points.foreach_set("co", pairs.ravel())
    linear = (
        bpy.types.Keyframe.bl_rna.properties["interpolation"].enum_items["LINEAR"].value
    )
    points.foreach_set("interpolation", [linear] * len(frames))


def transform_action(obj, frames, samples, name):
    obj.rotation_mode = "QUATERNION"
    bag = action_bag(obj, name)
    for lane in range(3):
        curve(bag, "location", lane, frames, samples[:, lane])
    for lane in range(4):
        curve(bag, "rotation_quaternion", lane, frames, samples[:, 3 + lane])
