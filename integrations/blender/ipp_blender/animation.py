"""Reusable action extraction for the standard Blender exporter."""

import math
from array import array
from .assets import clip_asset
from .sampling import SamplingSchedule, action_frames
from .exporter import identity, mesh
from .light_animation import export_light_action, sample_light_actions
from .mesh_animation import export_mesh_action, sample_mesh_actions


FIELDS = ("x", "y", "z", "qx", "qy", "qz", "qw", "sx", "sy", "sz")


def source_key(obj, action, scope, slot):
    return ("transform", obj.name, action.name, scope, slot.handle if slot else 0)


def encode_action(exporter, obj, action, fps, times, columns, poses):
    start, end = action.frame_range
    tracks = []
    for field in ("x", "y", "z", "sx", "sy", "sz"):
        values = columns[field]
        if max(values) - min(values) > 1e-6:
            tracks.append(
                {
                    "property": {"component": "Transform", "fields": [field]},
                    "keys": [
                        {"time": time, "value": {"kind": "f32", "value": value}}
                        for time, value in zip(times, values, strict=True)
                    ],
                }
            )
    rotations = list(
        zip(*(columns[field] for field in ("qx", "qy", "qz", "qw")), strict=True)
    )
    if any(
        sum(abs(a - b) for a, b in zip(rotations[0], q, strict=True)) > 1e-6
        for q in rotations[1:]
    ):
        tracks.append(
            {
                "property": {
                    "component": "Transform",
                    "fields": ["qx", "qy", "qz", "qw"],
                },
                "keys": [
                    {"time": time, "value": {"kind": "rotation", "value": q}}
                    for time, q in zip(times, rotations, strict=True)
                ],
            }
        )
    if obj.name in exporter.rigs:
        tracks.append(
            {
                "joints": list(range(len(exporter.rigs[obj.name]["bones"]))),
                "keys": [
                    {"time": time, "value": {"kind": "pose", "value": pose}}
                    for time, pose in zip(times, poses, strict=True)
                ],
            }
        )
    if not tracks:
        return []
    source = exporter.source(
        clip_asset({"duration": (end - start) / fps, "tracks": tracks}),
        "application/json",
        key=source_key(
            obj, action, exporter.action_scope, obj.animation_data.action_slot
        ),
    )
    return [
        {
            "id": f"{target}:action:{action.name}",
            "target": target,
            "source": source,
            "looping": True,
            "speed": 1,
            "autoplay": False,
        }
        for target in exporter.targets[obj.name]
    ]


def sample_action(exporter, obj, action, fps):
    start, end = action.frame_range
    if end <= start:
        return []
    count = max(2, math.ceil(end - start) + 1)
    times, poses = [], []
    columns = {field: array("f") for field in FIELDS}
    for index in range(count):
        value = start + (end - start) * index / (count - 1)
        exporter.scene.frame_set(math.floor(value), subframe=value % 1)
        exporter.checkpoint()
        times.append((value - start) / fps)
        transform = identity.transform(exporter, obj)
        for field in FIELDS:
            columns[field].append(transform[field])
        if obj.name in exporter.rigs:
            poses.append(mesh.poses(exporter, obj))
    return encode_action(exporter, obj, action, fps, times, columns, poses)


def sample_active_actions(exporter, objects, fps, schedule):
    """Register compatible active transforms and rigs in the shared scene pass.

    NLA/driver associations retain the existing isolated evaluation path. No actions
    or object bases change here, and the timeline is restored even on failure.
    Packed numeric columns bound scratch to 40 bytes per object/sample rather than
    keeping millions of Python transform dictionaries alive during a large bake.
    """
    groups = {}
    for obj in objects:
        data = obj.animation_data
        if (
            obj.name in exporter.targets
            and data
            and data.action
            and not data.nla_tracks
            and not data.drivers
        ):
            start, end = data.action.frame_range
            if end > start:
                groups.setdefault((start, end), []).append(obj)
    result = {}
    for (start, end), group in groups.items():
        schedule.add(
            _active_actions_group(exporter, group, start, end, fps, result),
            fractional=bool(start % 1 or end % 1),
        )
    return result


def _active_actions_group(exporter, group, start, end, fps, result):
    from .exporter.types import Unsupported

    samples = {obj.name: ({field: array("f") for field in FIELDS}, []) for obj in group}
    times = []
    failed = set()
    for value in action_frames(start, end):
        yield value
        times.append((value - start) / fps)
        for obj in group:
            if obj.name in failed:
                continue
            columns, poses = samples[obj.name]
            try:
                transform = identity.transform(exporter, obj)
                for field in FIELDS:
                    columns[field].append(transform[field])
                if obj.name in exporter.rigs:
                    poses.append(mesh.poses(exporter, obj))
            except (Unsupported, ValueError):
                # Isolated sampling below owns the existing error/diagnostic policy.
                failed.add(obj.name)
    for obj in group:
        if obj.name not in failed:
            columns, poses = samples[obj.name]
            result[obj.name] = encode_action(
                exporter,
                obj,
                obj.animation_data.action,
                fps,
                times,
                columns,
                poses,
            )


def prepare_actions(exporter, objects, schedule):
    fps = exporter.scene.render.fps / exporter.scene.render.fps_base
    return (
        sample_active_actions(exporter, objects, fps, schedule),
        sample_light_actions(exporter, objects, fps, schedule),
        sample_mesh_actions(exporter, objects, fps, schedule),
    )


def export_actions(exporter, objects, sampled=None):
    """Export active playback plus independently selectable active/stashed actions.

    NLA strips establish object/action/slot associations; unrelated global actions
    are never guessed onto a compatible-looking rig. Library clips ignore strip
    timing and blending. The active playback retains its evaluated NLA behavior.
    """
    from .exporter.types import Unsupported

    scene = exporter.scene
    frame, subframe = scene.frame_current, scene.frame_subframe
    fps = scene.render.fps / scene.render.fps_base
    playback, library = [], []
    if sampled is None:
        schedule = SamplingSchedule(exporter)
        sampled = prepare_actions(exporter, objects, schedule)
        schedule.run()
    sampled_active, sampled_lights, sampled_meshes = sampled
    try:
        for obj in objects:
            exporter.action_scope = "active"
            if obj.type == "LIGHT" and obj.name in exporter.targets:
                try:
                    light_playback, light_library = (
                        sampled_lights[obj.name]
                        if obj.name in sampled_lights
                        else export_light_action(exporter, obj, fps)
                    )
                    playback.extend(light_playback)
                    library.extend(light_library)
                except (Unsupported, ValueError) as error:
                    exporter.diagnostic("light-animation-unsupported", str(error), obj)
            if obj.name in exporter.mesh_poses:
                try:
                    shape_clips = (
                        sampled_meshes[obj.name]
                        if obj.name in sampled_meshes
                        else export_mesh_action(exporter, obj, fps)
                    )
                    playback.extend(shape_clips)
                    shape_data = obj.data.shape_keys.animation_data
                    library.extend(
                        {
                            "id": clip["id"]
                            + f":slot:{shape_data.action_slot.handle if shape_data.action_slot else 0}",
                            "name": shape_data.action.name,
                            "target": clip["target"],
                            "source": clip["source"],
                        }
                        for clip in shape_clips
                    )
                except (Unsupported, ValueError) as error:
                    exporter.diagnostic("mesh-animation-unsupported", str(error), obj)
            data = obj.animation_data
            if data is None or obj.name not in exporter.targets:
                continue
            active, slot = data.action, data.action_slot
            if obj.name in sampled_active:
                clips = sampled_active[obj.name]
                playback.extend(clips)
                library.extend(
                    {
                        "id": clip["id"] + f":slot:{slot.handle if slot else 0}",
                        "name": active.name,
                        "target": clip["target"],
                        "source": clip["source"],
                    }
                    for clip in clips
                )
                continue
            original_basis = obj.matrix_basis.copy()
            bone_basis = (
                {bone.name: bone.matrix_basis.copy() for bone in obj.pose.bones}
                if obj.type == "ARMATURE"
                else {}
            )
            muted = [(track, track.mute) for track in data.nla_tracks]
            if active is not None:
                if any(not track.mute for track in data.nla_tracks):
                    exporter.diagnostic(
                        "animation-nla",
                        "Active NLA composition is sampled as the current evaluated action range",
                        obj,
                    )
                playback.extend(sample_action(exporter, obj, active, fps))
            associated = []
            if active:
                associated.append((active, slot))
            for track in data.nla_tracks:
                for strip in track.strips:
                    if strip.type == "CLIP" and strip.action:
                        associated.append((strip.action, strip.action_slot))
            seen = set()
            try:
                exporter.action_scope = "library"
                for track, _ in muted:
                    track.mute = True
                for action, action_slot in associated:
                    identity = (action.name, action_slot.handle if action_slot else 0)
                    if identity in seen:
                        continue
                    seen.add(identity)
                    data.action = None
                    obj.matrix_basis = original_basis
                    for name, basis in bone_basis.items():
                        obj.pose.bones[name].matrix_basis = basis
                    data.action = action
                    if action_slot:
                        data.action_slot = action_slot
                    for clip in sample_action(exporter, obj, action, fps):
                        library.append(
                            {
                                "id": clip["id"] + f":slot:{identity[1]}",
                                "name": action.name,
                                "target": clip["target"],
                                "source": clip["source"],
                            }
                        )
            finally:
                data.action = active
                if slot:
                    data.action_slot = slot
                obj.matrix_basis = original_basis
                for name, basis in bone_basis.items():
                    obj.pose.bones[name].matrix_basis = basis
                for track, mute in muted:
                    track.mute = mute
                scene.frame_set(frame, subframe=subframe)
    finally:
        scene.frame_set(frame, subframe=subframe)
    return playback, library
