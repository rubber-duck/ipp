"""Synchronous main-thread scene export orchestration and shared snapshot state."""

import threading

import bpy

from . import appearance, identity, mesh, view
from .types import Unsupported


class Exporter:
    def __init__(self, publish, stream=None):
        from ..asset_plan import AssetPlan

        self.plan = (
            AssetPlan(stream.asset_store, stream.asset, stream.checkpoint)
            if stream
            else None
        )
        self.action_scope = "active"
        self.publish = publish
        self.stream = stream
        self.scene = bpy.context.scene
        self.unit = self.scene.unit_settings.scale_length
        self.diagnostics = []
        self.ids = {}
        self.rigs = {}
        self.entities = []
        self.targets = {}
        self.images = {}
        self.mesh_poses = {}

    def diagnostic(self, code, message, obj=None):
        item = {"code": code, "message": message}
        if obj is not None:
            item["entity"] = self.ids.get(obj.name, obj.name)
        self.diagnostics.append(item)

    def source(self, data, content_type="application/octet-stream", *, key=None):
        if self.plan:
            return self.plan.publish(data, content_type, key)
        source = self.publish(data, content_type)
        if self.stream:
            self.stream.asset(source, content_type)
        return source

    def asset(self, encode, *args, priority=0):
        if self.plan:
            return self.plan.defer(encode, args, priority=priority)
        return self.source(encode(*args))

    def checkpoint(self):
        if self.stream:
            self.stream.checkpoint()

    def animation(self, objects, sampled=None):
        from ..animation import export_actions

        return export_actions(self, objects, sampled)

    def declare_animation_sources(self, objects):
        from ..animation import source_key

        for obj in objects:
            data = obj.animation_data
            if data:
                associated = []
                if data.action:
                    associated.append((data.action, data.action_slot))
                for track in data.nla_tracks:
                    associated.extend(
                        (strip.action, strip.action_slot)
                        for strip in track.strips
                        if strip.type == "CLIP" and strip.action
                    )
                for action, slot in associated:
                    for scope in ("active", "library"):
                        self.plan.reserve(
                            source_key(obj, action, scope, slot), "application/json"
                        )
            if (
                obj.type == "LIGHT"
                and obj.data.animation_data
                and obj.data.animation_data.action
            ):
                self.plan.reserve(
                    ("light", obj.name, obj.data.animation_data.action.name),
                    "application/json",
                )
            if obj.type == "MESH" and obj.data.shape_keys:
                self.plan.reserve(
                    ("shape", obj.data.shape_keys.name), "application/json"
                )
            if obj.get("ipp_particles", "NATIVE") == "BAKED":
                for index, system in enumerate(obj.particle_systems):
                    if system.settings.type == "EMITTER":
                        self.plan.reserve(("particles", obj.name, index))

    def export(self, animation):
        objects = sorted(
            (
                obj
                for obj in self.scene.objects
                if obj.visible_get() and not obj.hide_render
            ),
            key=lambda obj: obj.name,
        )
        visible = {obj.name for obj in objects}
        included = {obj.name: obj for obj in objects}
        for obj in objects:
            for modifier in obj.modifiers:
                if (
                    modifier.type == "ARMATURE"
                    and modifier.show_viewport
                    and modifier.object
                ):
                    included[modifier.object.name] = modifier.object
        for obj in list(included.values()):
            parent = obj.parent
            while parent is not None:
                included[parent.name] = parent
                parent = parent.parent
        objects = sorted(included.values(), key=lambda obj: obj.name)
        identity.identify(self, objects)
        depsgraph = bpy.context.evaluated_depsgraph_get()
        for obj in sorted(objects, key=lambda obj: (obj.type != "ARMATURE", obj.name)):
            if obj.name not in self.ids:
                continue
            try:
                if obj.type == "ARMATURE":
                    results = [mesh.rig(self, obj)]
                elif obj.name not in visible or obj.type == "EMPTY":
                    results = [identity.base(self, obj)]
                elif obj.type == "MESH":
                    results = (
                        [identity.base(self, obj)]
                        if obj.particle_systems and not obj.show_instancer_for_render
                        else mesh.meshes(self, obj, depsgraph)
                    )
                elif obj.type == "CAMERA":
                    results = [view.camera(self, obj, depsgraph)]
                elif obj.type == "LIGHT":
                    results = [view.light(self, obj)]
                else:
                    self.diagnostic(
                        "object-unsupported",
                        f"Object type {obj.type} is not exported",
                        obj,
                    )
                    results = (
                        [identity.base(self, obj)]
                        if any(child.parent == obj for child in objects)
                        else []
                    )
                self.entities.extend(results)
                self.targets[obj.name] = [self.ids[obj.name]] if results else []
            except (Unsupported, ValueError) as error:
                self.rigs.pop(obj.name, None)
                self.diagnostic("unsupported", str(error), obj)
            self.checkpoint()
        # Invalid ancestors cannot leave dangling relationships or skinned consumers.
        while True:
            ids = {entity["id"] for entity in self.entities}
            invalid = [
                entity
                for entity in self.entities
                if (entity.get("parent") is not None and entity["parent"] not in ids)
                or ("skin" in entity and entity["skin"]["skeleton"] not in ids)
            ]
            if not invalid:
                break
            for entity in invalid:
                self.entities.remove(entity)
                self.diagnostic(
                    "parent-unsupported",
                    f"Omitted {entity['name']}: ancestor was not exported",
                )
        ids = {entity["id"] for entity in self.entities}
        self.targets = {
            name: targets
            for name, targets in self.targets.items()
            if targets and all(target in ids for target in targets)
        }
        self.mesh_poses = {
            name: value
            for name, value in self.mesh_poses.items()
            if all(target in ids for target in value[1])
        }
        from ..animation import prepare_actions
        from ..particle_bake import prepare_particles
        from ..particles import export_particles
        from ..sampling import SamplingSchedule

        particle_objects = [
            obj
            for obj in objects
            if obj.name in self.ids and obj.name in visible and obj.particle_systems
        ]
        schedule = SamplingSchedule(self)
        sampled = prepare_actions(self, objects, schedule) if animation else None
        self.particle_bakes = prepare_particles(self, particle_objects, schedule)
        if self.plan:
            self.declare_animation_sources(objects)
        else:
            schedule.run()
        particle_animations = []
        particle_clips = []
        for obj in particle_objects:
            effects, animations, clips = export_particles(self, obj)
            self.entities.extend(effects)
            particle_animations.extend(animations)
            particle_clips.extend(clips)
        if self.plan:
            self.stream.asset_index(self.plan.index())
            self.stream.entities(self.entities, chained=True)
            self.stream.complete_entities()
            self.plan.produce()
            schedule.run()
        result = {"entities": self.entities, "diagnostics": self.diagnostics}
        try:
            result["ambient_light"] = appearance.ambient_light(self)
        except Unsupported as error:
            result["ambient_light"] = [0.0, 0.0, 0.0]
            self.diagnostic("world-unsupported", str(error))
        if self.scene.camera and self.scene.camera.name in self.targets:
            result["active_camera"] = self.ids[self.scene.camera.name]
        if animation:
            try:
                result["animations"], result["clips"] = self.animation(objects, sampled)
            except (Unsupported, ValueError) as error:
                self.diagnostic("animation-unsupported", str(error))
        if particle_animations:
            result.setdefault("animations", []).extend(particle_animations)
            result.setdefault("clips", []).extend(particle_clips)
        if self.plan:
            self.plan.produce()
            self.plan.finish()
            result["assets"] = self.plan.index()
        return result


def export_scene(publish, *, animation=True, stream=None):
    """Publish immutable bytes and return a JSON-compatible snapshot on the main thread."""
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Blender export must run on the main thread")
    exporter = Exporter(publish, stream)
    try:
        return exporter.export(animation)
    except BaseException as error:
        if exporter.plan:
            exporter.plan.finish(error)
        raise
