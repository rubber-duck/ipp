"""Export the scene and its application-owned gait origins/source-pose references."""

import json
import math
import runpy
import sys
from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / "integrations/blender"))
from ipp_blender.exporter.types import BASIS


def source_pose(rig, bones):
    """Sample Blender directly, independently of the runtime's clip evaluator."""
    result = []
    for bone in bones:
        matrix = rig.pose.bones[bone.name].matrix.copy()
        if bone.parent:
            matrix = rig.pose.bones[bone.parent.name].matrix.inverted() @ matrix
        matrix = BASIS @ matrix @ BASIS.inverted()
        matrix.translation *= bpy.context.scene.unit_settings.scale_length
        location, rotation, scale = matrix.decompose()
        result.append(
            {
                "translation": list(location),
                "rotation": [rotation.x, rotation.y, rotation.z, rotation.w],
                "scale": list(scale),
            }
        )
    return result


def main():
    # Keep the standard exporter as the sole scene/asset translation path.
    runpy.run_path(
        str(ROOT / "integrations/blender/export_scene.py"), run_name="__main__"
    )
    output = Path(sys.argv[sys.argv.index("--") + 1])
    scene = bpy.context.scene
    rig = bpy.data.objects["platformer-rig"]
    bones = []

    def visit(bone):
        bones.append(bone)
        for child in sorted(bone.children, key=lambda value: value.name):
            visit(child)

    for bone in sorted(
        (bone for bone in rig.data.bones if bone.parent is None),
        key=lambda value: value.name,
    ):
        visit(bone)
    data = rig.animation_data
    active, slot = data.action, data.action_slot
    frame, subframe = scene.frame_current, scene.frame_subframe
    muted = [(track, track.mute) for track in data.nla_tracks]
    original_basis = rig.matrix_basis.copy()
    bone_basis = {bone.name: bone.matrix_basis.copy() for bone in rig.pose.bones}
    baseline = source_pose(rig, bones)
    origins, references = {}, {}
    try:
        for track, _ in muted:
            track.mute = True
        for mode, name in (
            ("walk", "Platformer_Walk"),
            ("run", "Platformer_Run"),
            ("crawl", "Platformer_Crawl"),
        ):
            action = bpy.data.actions[name]
            data.action = None
            rig.matrix_basis = original_basis
            for name, basis in bone_basis.items():
                rig.pose.bones[name].matrix_basis = basis
            data.action = action
            data.action_slot = action.slots[0]
            start, end = action.frame_range
            samples = []
            for phase in (0, 0.25, 0.5):
                value = start + (end - start) * phase
                scene.frame_set(math.floor(value), subframe=value % 1)
                bpy.context.view_layer.update()
                samples.append(
                    {
                        "time": (value - start)
                        / (scene.render.fps / scene.render.fps_base),
                        "pose": source_pose(rig, bones),
                    }
                )
            origins[mode] = samples[0]["pose"]
            references[mode] = samples
    finally:
        data.action = active
        data.action_slot = slot
        rig.matrix_basis = original_basis
        for name, basis in bone_basis.items():
            rig.pose.bones[name].matrix_basis = basis
        for track, mute in muted:
            track.mute = mute
        scene.frame_set(frame, subframe=subframe)
    (output / "pose-origins.json").write_text(
        json.dumps(
            {
                "version": 1,
                "baseline": baseline,
                "origins": origins,
                "references": references,
            },
            indent=2,
        )
        + "\n"
    )


main()
