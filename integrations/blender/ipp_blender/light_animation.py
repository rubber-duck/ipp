"""Sample active light-data actions into ordinary Light property clips."""

import math

from .assets import clip_asset
from .sampling import action_frames


def active_action(obj):
    from .exporter.types import Unsupported

    light = obj.data
    data = light.animation_data
    if data is None:
        return None
    if data.drivers or any(not track.mute for track in data.nla_tracks):
        raise Unsupported(
            "Light drivers and active NLA composition are unsupported; use an active light-data action"
        )
    action = data.action
    if action is None:
        return None
    start, end = action.frame_range
    if end <= start:
        return None
    return action


def sample(light, time):
    from .exporter.types import Unsupported

    intensity = light.get("ipp_intensity")
    if intensity is None:
        intensity = (
            light.energy if light.type == "SUN" else light.energy / (4 * math.pi)
        )
    if not math.isfinite(intensity) or intensity < 0:
        raise Unsupported("Animated light intensity must be finite and nonnegative")
    return {
        "time": time,
        "intensity": intensity,
        "r": light.color[0],
        "g": light.color[1],
        "b": light.color[2],
    }


def export_light_action(exporter, obj, fps):
    action = active_action(obj)
    if action is None:
        return [], []
    start, end = action.frame_range
    scene = exporter.scene
    original = (scene.frame_current, scene.frame_subframe)
    samples = []
    try:
        count = max(2, math.ceil(end - start) + 1)
        for index in range(count):
            value = start + (end - start) * index / (count - 1)
            scene.frame_set(math.floor(value), subframe=value % 1)
            exporter.checkpoint()
            samples.append(sample(obj.data, (value - start) / fps))
    finally:
        scene.frame_set(original[0], subframe=original[1])
    return encode_action(exporter, obj, action, fps, samples)


def sample_light_actions(exporter, objects, fps, schedule):
    """Evaluate matching light action ranges once, without changing action bindings.

    Failed or unsupported lights keep the isolated path and its per-object diagnostics.
    Only detached property samples survive a scene evaluation.
    """
    from .exporter.types import Unsupported

    groups = {}
    for obj in objects:
        if obj.type != "LIGHT" or obj.name not in exporter.targets:
            continue
        try:
            action = active_action(obj)
        except Unsupported:
            continue
        if action is not None:
            groups.setdefault(tuple(action.frame_range), []).append(obj)
    result = {}
    for (start, end), group in groups.items():
        schedule.add(
            _light_actions_group(exporter, group, start, end, fps, result),
            fractional=bool(start % 1 or end % 1),
        )
    return result


def _light_actions_group(exporter, group, start, end, fps, result):
    from .exporter.types import Unsupported

    samples = {obj.name: [] for obj in group}
    failed = set()
    for value in action_frames(start, end):
        yield value
        for obj in group:
            if obj.name in failed:
                continue
            try:
                samples[obj.name].append(sample(obj.data, (value - start) / fps))
            except (Unsupported, ValueError):
                failed.add(obj.name)
    for obj in group:
        if obj.name not in failed:
            try:
                result[obj.name] = encode_action(
                    exporter,
                    obj,
                    obj.data.animation_data.action,
                    fps,
                    samples[obj.name],
                )
            except (Unsupported, ValueError):
                # The ordinary per-light path owns error reporting.
                pass


def encode_action(exporter, obj, action, fps, samples):
    start, end = action.frame_range
    data = obj.data.animation_data
    tracks = []
    for field in ("intensity", "r", "g", "b"):
        if max(s[field] for s in samples) - min(s[field] for s in samples) <= 1e-6:
            continue
        tracks.append(
            {
                "property": {"component": "Light", "fields": [field]},
                "keys": [
                    {"time": s["time"], "value": {"kind": "f32", "value": s[field]}}
                    for s in samples
                ],
            }
        )
    if not tracks:
        return [], []
    source = exporter.source(
        clip_asset({"duration": (end - start) / fps, "tracks": tracks}),
        "application/json",
        key=("light", obj.name, action.name),
    )
    target = exporter.ids[obj.name]
    identifier = f"{target}:light-action:{action.name}"
    slot = data.action_slot.handle if data.action_slot else 0
    return (
        [
            {
                "id": identifier,
                "target": target,
                "source": source,
                "looping": True,
                "speed": 1,
                "autoplay": False,
            }
        ],
        [
            {
                "id": identifier + f":slot:{slot}",
                "name": action.name,
                "target": target,
                "source": source,
            }
        ],
    )
