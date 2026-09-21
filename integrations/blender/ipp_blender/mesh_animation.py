"""Sample supported shape-key actions into shared immutable mesh-pose clips."""

import math

from .assets import clip_asset
from .sampling import action_frames
from .exporter.mesh import shape_weight
from .exporter.types import Unsupported


def active_action(exporter, obj):
    key, _ = exporter.mesh_poses[obj.name]
    data = obj.data.shape_keys.animation_data
    if data is None or key.mute:
        return None
    if data.drivers or any(not track.mute for track in data.nla_tracks):
        raise Unsupported(
            "Mesh pose animation requires an active action without drivers or NLA composition"
        )
    if data.action is None:
        return None
    start, end = data.action.frame_range
    if end <= start:
        return None
    return data.action


def encode_action(exporter, obj, action, source):
    _, targets = exporter.mesh_poses[obj.name]
    return [
        {
            "id": f"{target}:shape-action:{action.name}",
            "target": target,
            "source": source,
            "looping": True,
            "speed": 1,
            "autoplay": False,
        }
        for target in targets
    ]


def publish_samples(exporter, start, end, fps, samples, data):
    return exporter.source(
        clip_asset(
            {
                "duration": (end - start) / fps,
                "tracks": [
                    {
                        "property": {"component": "MeshPose", "fields": ["weight"]},
                        "keys": samples,
                    }
                ],
            }
        ),
        "application/json",
        key=("shape", data.name),
    )


def export_mesh_action(exporter, obj, fps):
    action = active_action(exporter, obj)
    if action is None:
        return []
    key, _ = exporter.mesh_poses[obj.name]
    start, end = action.frame_range
    count = max(2, math.ceil(end - start) + 1)
    samples = []
    for index in range(count):
        value = start + (end - start) * index / (count - 1)
        exporter.scene.frame_set(math.floor(value), subframe=value % 1)
        exporter.checkpoint()
        samples.append(
            {
                "time": (value - start) / fps,
                "value": {"kind": "f32", "value": shape_weight(key)},
            }
        )
    return encode_action(
        exporter,
        obj,
        action,
        publish_samples(exporter, start, end, fps, samples, obj.data.shape_keys),
    )


def sample_mesh_actions(exporter, objects, fps, schedule):
    """Share timeline evaluations by range and samples by shape-key datablock.

    Shape-key values belong to the datablock, not each object instance. Distinct
    datablocks keep independent samples even if they use the same action/slot.
    Unsupported bindings and sampled failures retain per-object diagnostics.
    """
    groups = {}
    for obj in objects:
        if obj.name not in exporter.mesh_poses:
            continue
        try:
            action = active_action(exporter, obj)
        except Unsupported:
            continue
        if action is not None:
            groups.setdefault(tuple(action.frame_range), []).append(obj)
    result = {}
    for (start, end), group in groups.items():
        schedule.add(
            _mesh_actions_group(exporter, group, start, end, fps, result),
            fractional=bool(start % 1 or end % 1),
        )
    return result


def _mesh_actions_group(exporter, group, start, end, fps, result):
    from .exporter.types import Unsupported

    # Live Blender references stay inside this synchronous extraction.
    keys = {obj.data.shape_keys: exporter.mesh_poses[obj.name][0] for obj in group}
    samples = {data: [] for data in keys}
    failed = set()
    for value in action_frames(start, end):
        yield value
        for data, key in keys.items():
            if data in failed:
                continue
            try:
                samples[data].append(
                    {
                        "time": (value - start) / fps,
                        "value": {"kind": "f32", "value": shape_weight(key)},
                    }
                )
            except (Unsupported, ValueError):
                failed.add(data)
    sources = {}
    for data in keys:
        if data in failed:
            continue
        try:
            sources[data] = publish_samples(
                exporter, start, end, fps, samples[data], data
            )
        except (Unsupported, ValueError):
            pass
    for obj in group:
        source = sources.get(obj.data.shape_keys)
        if source is not None:
            result[obj.name] = encode_action(
                exporter, obj, obj.data.shape_keys.animation_data.action, source
            )
