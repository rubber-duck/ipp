"""Particle effect entity, appearance and playback orchestration."""

from .assets import clip_asset
from .exporter import appearance, identity
from .exporter.types import Unsupported
from .particle_bake import bake
from .particle_native import native, static_mesh


def export_particles(exporter, obj):
    entities, animations, clips = [], [], []
    mode = obj.get("ipp_particles", "NATIVE")
    for index, system in enumerate(obj.particle_systems):
        try:
            effect_animations, effect_clips = [], []
            if mode not in {"NATIVE", "BAKED"}:
                raise Unsupported("ipp_particles must be NATIVE or BAKED")
            if system.settings.type != "EMITTER":
                raise Unsupported("Hair particle systems are not particle effects")
            identifier = f"{exporter.ids[obj.name]}:particles:{system.name}"
            effect = {
                "id": identifier,
                "name": f"{obj.name}/{system.name}",
                "transform": {
                    "x": 0,
                    "y": 0,
                    "z": 0,
                    "qx": 0,
                    "qy": 0,
                    "qz": 0,
                    "qw": 1,
                    "sx": 1,
                    "sy": 1,
                    "sz": 1,
                },
            }
            if mode == "NATIVE":
                effect["transform"] = identity.transform(exporter, obj)
                effect["particle_emitter"] = native(exporter, obj, system)
            else:
                if exporter.plan:
                    fps = exporter.scene.render.fps / exporter.scene.render.fps_base
                    baked = (
                        exporter.plan.reserve(("particles", obj.name, index)),
                        (exporter.scene.frame_end - exporter.scene.frame_start) / fps,
                    )
                else:
                    baked = exporter.particle_bakes.pop((obj.name, index), None)
                if isinstance(baked, Exception):
                    raise baked
                source, duration = (
                    baked if baked is not None else bake(exporter, obj, index)
                )
                effect["particle_playback"] = {"source": source, "time": 0}
                clip = {
                    "duration": duration,
                    "tracks": [
                        {
                            "property": {
                                "component": "ParticlePlayback",
                                "fields": ["time"],
                            },
                            "keys": [
                                {"time": 0, "value": {"kind": "f32", "value": 0}},
                                {
                                    "time": duration,
                                    "value": {"kind": "f32", "value": duration},
                                },
                            ],
                        }
                    ],
                }
                effect_animations.append(
                    {
                        "id": identifier + ":time",
                        "target": identifier,
                        "source": exporter.source(clip_asset(clip), "application/json"),
                        "looping": True,
                        "autoplay": True,
                    }
                )
                effect_clips.append(
                    {
                        "id": identifier + ":time",
                        "name": system.name,
                        "target": identifier,
                        "source": effect_animations[-1]["source"],
                    }
                )
            settings = system.settings
            if settings.render_type == "OBJECT" and settings.instance_object:
                instance = settings.instance_object
                effect["particle_mesh"] = {
                    "source": static_mesh(exporter, instance, instance=True)
                }
                effect.update(
                    appearance.material(
                        exporter,
                        instance.active_material,
                        instance,
                        bool(instance.data.uv_layers.active),
                    )
                )
            elif settings.render_type == "HALO":
                slot = settings.material - 1
                material = (
                    obj.material_slots[slot].material
                    if 0 <= slot < len(obj.material_slots)
                    else None
                )
                surface = (
                    appearance.material(exporter, material, obj, False)["material"]
                    if material is not None
                    else {"r": 1, "g": 1, "b": 1}
                )
                blend = settings.get("ipp_blend", "ALPHA")
                if blend not in {"ALPHA", "ADDITIVE"}:
                    raise Unsupported("Particle ipp_blend must be ALPHA or ADDITIVE")
                effect["particle_sprite"] = {
                    "r": surface["r"],
                    "g": surface["g"],
                    "b": surface["b"],
                    "blend": int(blend == "ADDITIVE"),
                    "opacity": 1,
                    "end_opacity": 0,
                }
            else:
                raise Unsupported(
                    "Particle render type requires one object or a sprite"
                )
            entities.append(effect)
            animations.extend(effect_animations)
            clips.extend(effect_clips)
        except (Unsupported, ValueError) as error:
            exporter.diagnostic("particle-unsupported", str(error), obj)
    return entities, animations, clips
