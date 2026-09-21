"""Deterministic Bullet/Rigify benchmark authoring; run in a fresh background Blender.

blender -b --factory-startup --python-exit-code 1 --python tests/blender/stress_scene.py -- --output target/stress-benchmark/full --grid 100
The saved physics-source.blend retains rigid bodies and Rigify controls. benchmark.blend
contains their sampled actions, compact deformation rigs and native/baked particle systems.
"""

import argparse
import json
import math
import random
import sys
import time
from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).parent))
from stress_authoring import (
    identify,
    material,
    object_from_mesh,
    action_bag,
    curve,
    transform_action,
)
from stress_features import add_pose_panels, animate_shape, sample_pose_probes


def rigify_walkers(scene, count, frames, fps, extent, palette):
    bpy.ops.preferences.addon_enable(module="rigify")
    bpy.ops.object.armature_basic_human_metarig_add()
    metarig = identify(bpy.context.object, "authoring-metarig")
    bpy.ops.pose.rigify_generate()
    source = identify(bpy.context.object, "authoring-rigify-controls")
    for side in ("L", "R"):
        for limb in ("thigh", "upper_arm"):
            source.pose.bones[f"{limb}_parent.{side}"]["IK_FK"] = 1.0
    # Original Rigify controls remain authored; only evaluated deformation is baked.
    bag = action_bag(source, "Rigify authored walk")
    t = (frames - 1) / fps
    for side, phase in (("L", 0), ("R", math.pi)):
        wave = np.sin(t * math.tau + phase)
        for control, values in (
            ("thigh_fk", 0.45 * wave),
            ("shin_fk", -0.65 * np.maximum(-wave, 0)),
            ("foot_fk", -0.2 * wave),
            ("upper_arm_fk", -0.32 * wave),
            ("forearm_fk", -0.2 + 0.12 * wave),
        ):
            bone = source.pose.bones[f"{control}.{side}"]
            bone.rotation_mode = "XYZ"
            curve(bag, bone.path_from_id("rotation_euler"), 0, frames, values)
    torso = source.pose.bones["torso"]
    curve(
        bag,
        torso.path_from_id("location"),
        2,
        frames,
        0.035 * (1 - np.cos(t * math.tau * 2)),
    )
    selected = [
        b
        for b in source.data.bones
        if b.use_deform and not b.name.startswith(("DEF-breast", "DEF-pelvis"))
    ]
    assert len(selected) <= 32
    names = {b.name for b in selected}
    parents = {}
    for bone in selected:
        parent = bone.parent
        while parent and parent.name not in names:
            parent = parent.parent
        parents[bone.name] = parent.name if parent else None
    armature = bpy.data.armatures.new("Compact Rigify deformation")
    compact = identify(bpy.data.objects.new("walker-00-rig", armature), "walker-00-rig")
    scene.collection.objects.link(compact)
    bpy.ops.object.select_all(action="DESELECT")
    compact.select_set(True)
    bpy.context.view_layer.objects.active = compact
    bpy.ops.object.mode_set(mode="EDIT")
    for bone in selected:
        target = armature.edit_bones.new(bone.name)
        target.head = bone.head_local
        target.tail = bone.tail_local
        target.matrix = bone.matrix_local
    for name, parent in parents.items():
        if parent:
            armature.edit_bones[name].parent = armature.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    samples = np.empty((len(frames), len(selected), 10), dtype=np.float32)
    reference = {}
    for index, frame in enumerate(frames):
        scene.frame_set(int(frame))
        for j, bone in enumerate(selected):
            matrix = source.pose.bones[bone.name].matrix.copy()
            parent = parents[bone.name]
            local = (
                source.pose.bones[parent].matrix.inverted() @ matrix
                if parent
                else matrix
            )
            rest = armature.bones[bone.name].matrix_local.copy()
            if parent:
                rest = armature.bones[parent].matrix_local.inverted() @ rest
            location, rotation, scale = (rest.inverted() @ local).decompose()
            samples[index, j] = (*location, *rotation, *scale)
        if index in (0, len(frames) // 4, len(frames) // 2, len(frames) - 1):
            reference[int(frame)] = [
                list(v) for b in selected for v in source.pose.bones[b.name].matrix
            ]
    bag = action_bag(compact, "Rigify baked deformation walk")
    for j, bone in enumerate(selected):
        target = compact.pose.bones[bone.name]
        target.rotation_mode = "QUATERNION"
        for path, offset, lanes in (
            ("location", 0, 3),
            ("rotation_quaternion", 3, 4),
            ("scale", 7, 3),
        ):
            for lane in range(lanes):
                curve(
                    bag,
                    target.path_from_id(path),
                    lane,
                    frames,
                    samples[:, j, offset + lane],
                )
    maximum_error = 0.0
    for frame, expected in reference.items():
        scene.frame_set(frame)
        actual = [list(v) for b in selected for v in compact.pose.bones[b.name].matrix]
        maximum_error = max(
            maximum_error, float(np.max(np.abs(np.array(actual) - expected)))
        )
    assert maximum_error < 1e-4, maximum_error
    # A complete low-poly humanoid: torso/head and tapered limbs share one skinned mesh.
    vertices, faces, groups = [], [], []
    for bone in selected:
        if bone.name.startswith(("DEF-shoulder", "DEF-spine.004", "DEF-spine.005")):
            continue
        length = bone.length
        radius = (
            0.065
            if any(s in bone.name for s in ("arm", "hand", "shin", "foot", "toe"))
            else 0.1
        )
        if bone.name.startswith("DEF-spine"):
            radius = 0.14 if bone.name != "DEF-spine.006" else 0.15
        start = len(vertices)
        for y, width in ((0, radius), (length, radius * 0.85)):
            for x, z in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                vertices.append(
                    tuple(bone.matrix_local @ Vector((x * width, y, z * width * 0.7)))
                )
                groups.append(bone.name)
        faces.extend(
            tuple(start + i for i in face)
            for face in (
                (0, 3, 2, 1),
                (4, 5, 6, 7),
                (0, 1, 5, 4),
                (1, 2, 6, 5),
                (2, 3, 7, 6),
                (3, 0, 4, 7),
            )
        )
    mesh = bpy.data.meshes.new("Shared Rigify humanoid")
    mesh.from_pydata(vertices, [], faces)
    mesh.materials.append(palette[0])
    for number in range(count):
        rig = compact if number == 0 else compact.copy()
        if number:
            scene.collection.objects.link(rig)
        identify(rig, f"walker-{number:02}-rig")
        rig.location = ((number - (count - 1) / 2) * 5, -extent - 9, 0)
        rig.scale = (3, 3, 3)
        body = object_from_mesh(
            mesh,
            f"walker-{number:02}-human",
            rig.location,
            [palette[number % len(palette)]],
        )
        body.scale = rig.scale
        if number == 0:
            body.shape_key_add(name="Basis")
            shape = body.shape_key_add(name="Breathing silhouette")
            for point in shape.data:
                point.co.x *= 1.35
            animate_shape(body, shape, frames, fps, 0)
        for name in names:
            indices = [i for i, group in enumerate(groups) if group == name]
            if indices:
                body.vertex_groups.new(name=name).add(indices, 1, "REPLACE")
        modifier = body.modifiers.new("Rigify deformation", "ARMATURE")
        modifier.object = rig
        modifier.use_deform_preserve_volume = False
        # Root route is a parent so all instances reuse identical local pose content.
        root = identify(
            bpy.data.objects.new(f"walker-{number:02}-route", None),
            f"walker-{number:02}-route",
        )
        scene.collection.objects.link(root)
        rig.parent = root
        body.parent = root
        root_bag = action_bag(root, f"Walker route {number}")
        curve(root_bag, "location", 0, frames, np.sin(t * math.tau / 10 + number) * 3)
    metarig.hide_render = source.hide_render = True
    metarig.hide_set(True)
    source.hide_set(True)
    scene.frame_set(1)
    return {
        "source_bones": len(source.data.bones),
        "export_bones": len(selected),
        "walkers": count,
        "max_bake_matrix_error": maximum_error,
    }


def add_particles(scene, cube_mesh, palette, extent, count, end):
    for side, mode in ((-1, "BAKED"), (1, "NATIVE")):
        bpy.ops.mesh.primitive_plane_add(size=3, location=(side * (extent + 7), 0, 1))
        emitter = identify(bpy.context.object, f"particles-{mode.lower()}")
        emitter["ipp_particles"] = mode
        emitter.show_instancer_for_render = False
        bpy.ops.object.particle_system_add()
        system = emitter.particle_systems[0]
        system.seed = 7349
        settings = system.settings
        settings.count = count
        settings.frame_start = 1
        settings.frame_end = end
        settings.lifetime = 72
        settings.lifetime_random = 0.2
        settings.normal_factor = 10
        settings.particle_size = 0.18
        settings.size_random = 0.4
        settings.render_type = "OBJECT"
        prototype = object_from_mesh(
            cube_mesh,
            f"particle-{mode.lower()}-cube",
            (0, 0, -30),
            [palette[0 if side < 0 else 3]],
        )
        prototype.hide_render = True
        settings.instance_object = prototype
        settings.use_rotations = mode == "BAKED"
        if mode == "BAKED":
            settings.brownian_factor = 0.7
            settings.rotation_mode = "VEL"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--grid", type=int, default=100)
    parser.add_argument("--seconds", type=int, default=10)
    parser.add_argument("--fps", type=int, default=24)
    parser.add_argument("--particles", type=int, default=2000)
    parser.add_argument("--humans", type=int, default=4)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])
    assert 2 <= args.grid <= 1000 and args.seconds > 0 and args.fps > 0
    assert bpy.app.background, "Run this generator in its own background process"
    args.output.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    scene = bpy.context.scene
    scene.frame_start, scene.frame_end = 1, args.seconds * args.fps + 1
    scene.render.fps = args.fps
    scene.render.resolution_x, scene.render.resolution_y = 960, 640
    scene.render.resolution_percentage = 100
    frames = np.arange(1, scene.frame_end + 1, dtype=np.float32)
    extent = args.grid * 1.3 / 2
    palette = [
        material(f"Cube palette {i}", color)
        for i, color in enumerate(
            (
                (0.1, 0.45, 0.7),
                (0.7, 0.22, 0.1),
                (0.4, 0.65, 0.12),
                (0.65, 0.15, 0.4),
                (0.55, 0.45, 0.18),
                (0.13, 0.65, 0.6),
                (0.3, 0.25, 0.65),
                (0.65, 0.65, 0.7),
            )
        )
    ]
    bpy.ops.mesh.primitive_cube_add(size=1)
    template = bpy.context.object
    cube_mesh = template.data
    cube_mesh.name = "Shared stress cube"
    cube_mesh.materials.append(palette[0])
    bpy.data.objects.remove(template, do_unlink=True)
    rng = random.Random(7349)
    cubes = []
    for row in range(args.grid):
        for column in range(args.grid):
            obj = object_from_mesh(
                cube_mesh,
                f"drop-{row:03}-{column:03}",
                (
                    (column - (args.grid - 1) / 2) * 1.3,
                    (row - (args.grid - 1) / 2) * 1.3,
                    3 + rng.random() * 7,
                ),
                [palette[(row + column) % len(palette)]],
            )
            obj.rotation_euler = (
                rng.uniform(-0.2, 0.2),
                rng.uniform(-0.2, 0.2),
                rng.uniform(-0.4, 0.4),
            )
            obj.select_set(True)
            cubes.append(obj)
    bpy.context.view_layer.objects.active = cubes[0]
    bpy.ops.rigidbody.objects_add(type="ACTIVE")
    for obj in cubes:
        rb = obj.rigid_body
        rb.collision_shape = "BOX"
        rb.mass = 1
        rb.friction = 0.6
        rb.restitution = 0.25
        rb.use_margin = True
        rb.collision_margin = 0.005
    bpy.ops.object.select_all(action="DESELECT")
    floor = object_from_mesh(
        cube_mesh,
        "flat-ground-plane",
        (0, 0, -0.5),
        [material("Ground", (0.12, 0.14, 0.17))],
    )
    floor.scale = (extent * 2 + 30, extent * 2 + 35, 1)
    floor.select_set(True)
    bpy.context.view_layer.objects.active = floor
    bpy.ops.rigidbody.object_add(type="PASSIVE")
    floor.rigid_body.collision_shape = "BOX"
    floor.rigid_body.friction = 0.6
    scene.rigidbody_world.substeps_per_frame = 3
    scene.rigidbody_world.solver_iterations = 12
    scene.rigidbody_world.point_cache.frame_end = scene.frame_end
    bpy.ops.object.select_all(action="DESELECT")
    rig = (
        rigify_walkers(scene, args.humans, frames, args.fps, extent, palette)
        if args.humans
        else {}
    )
    for index in range(20):
        data = bpy.data.lights.new(
            f"Parented light {index:02}", "SPOT" if index % 5 == 0 else "POINT"
        )
        light = identify(
            bpy.data.objects.new(f"parented-light-{index:02}", data),
            f"parented-light-{index:02}",
        )
        scene.collection.objects.link(light)
        light.parent = cubes[min(len(cubes) - 1, index * len(cubes) // 20)]
        light.location = (0, 0, 2.5)
        data.energy = 700
        data.color = tuple(
            palette[index % len(palette)]
            .node_tree.nodes.get("Principled BSDF")
            .inputs["Base Color"]
            .default_value[:3]
        )
        data["ipp_range"] = 16.0
        if data.type == "SPOT":
            data.spot_size = 1.3
        data.use_shadow = index % 5 == 0
        data["ipp_intensity"] = 40.0
        bag = action_bag(data, f"Light pulse {index:02}")
        curve(
            bag,
            '["ipp_intensity"]',
            0,
            frames,
            40 + 30 * np.sin((frames - 1) / args.fps * 2 + index),
        )
        for lane in range(3):
            curve(
                bag,
                "color",
                lane,
                frames,
                0.5 + 0.45 * np.sin((frames - 1) / args.fps + index + lane * 2),
            )
    sun_data = bpy.data.lights.new("Fill sun", "SUN")
    sun = identify(bpy.data.objects.new("fill-sun", sun_data), "fill-sun")
    scene.collection.objects.link(sun)
    sun.rotation_euler = (0.4, -0.5, -0.3)
    sun_data["ipp_intensity"] = 1.3
    scene.world.use_nodes = True
    scene.world.node_tree.nodes.get("Background").inputs["Color"].default_value = (
        0.15,
        0.18,
        0.22,
        1,
    )
    scene.world.node_tree.nodes.get("Background").inputs["Strength"].default_value = 0.5
    camera = identify(
        bpy.data.objects.new("benchmark-camera", bpy.data.cameras.new("Stress camera")),
        "benchmark-camera",
    )
    scene.collection.objects.link(camera)
    camera.data.clip_end = 1000
    camera.data.lens = 38
    scene.camera = camera
    samples = []
    for frame in frames:
        angle = (frame - 1) / args.fps / args.seconds * 0.35
        position = Vector(
            (
                math.sin(angle) * (extent + 45),
                -(extent + 40) * math.cos(angle),
                extent + 35,
            )
        )
        target = Vector((0, -extent * 0.15, 0))
        samples.append((*position, *(target - position).to_track_quat("-Z", "Y")))
    transform_action(camera, frames, np.array(samples), "Camera orbit")
    # Separate color groups make culling/sorting and deep composed transforms visible.
    for chain in range(2):
        parent = cubes[len(cubes) // 2 + chain]
        for depth in range(12):
            child = object_from_mesh(
                cube_mesh,
                f"attachment-{chain}-{depth:02}",
                (0, 0, 0.8),
                [palette[chain]],
            )
            child.scale = (0.85, 0.85, 0.85)
            child.parent = parent
            parent = child
    panels = add_pose_panels(
        scene, frames, args.fps, extent, 4 if args.grid < 100 else 32
    )
    # Fading alpha sprites exercise supported transparency and instance sorting.
    bpy.ops.mesh.primitive_plane_add(size=3, location=(0, extent + 3, 1))
    alpha = identify(bpy.context.object, "alpha-sprite-emitter")
    alpha.show_instancer_for_render = False
    alpha.data.materials.append(palette[5])
    bpy.ops.object.particle_system_add()
    alpha_system = alpha.particle_systems[0]
    alpha_system.seed = 901
    settings = alpha_system.settings
    settings.count = max(100, args.particles // 4)
    settings.frame_start = 1
    settings.frame_end = scene.frame_end
    settings.lifetime = 72
    settings.normal_factor = 8
    settings.particle_size = 0.45
    settings.render_type = "HALO"
    settings.use_rotations = False
    add_particles(scene, cube_mesh, palette, extent, args.particles, scene.frame_end)
    scene.frame_set(1)
    bpy.context.view_layer.update()
    bpy.ops.wm.save_as_mainfile(
        filepath=str((args.output / "physics-source.blend").resolve()), compress=True
    )
    print(f"STRESS created {len(cubes)} cubes, sampling Bullet", flush=True)
    # One sequential physics evaluation per frame; retain just TRQ lanes.
    samples = np.empty((len(frames), len(cubes), 7), dtype=np.float32)
    for index, frame in enumerate(frames):
        scene.frame_set(int(frame))
        for j, obj in enumerate(cubes):
            matrix = obj.matrix_world
            samples[index, j] = (*matrix.to_translation(), *matrix.to_quaternion())
        if index % args.fps == 0:
            print(
                f"STRESS physics {index}/{len(frames) - 1}, elapsed {time.perf_counter() - started:.1f}s",
                flush=True,
            )
    assert float(np.mean(samples[0, :, 2] - samples[-1, :, 2])) > 1
    # Only task-owned active cubes lose simulation; the source file retains it.
    bpy.ops.object.select_all(action="DESELECT")
    for obj in cubes:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = cubes[0]
    bpy.ops.rigidbody.objects_remove()
    for j, obj in enumerate(cubes):
        values = samples[:, j]
        for index in range(1, len(values)):
            if np.dot(values[index - 1, 3:], values[index, 3:]) < 0:
                values[index, 3:] *= -1
        transform_action(obj, frames, values, f"Bullet drop {j:05}")
    probes = []
    for frame_index in (0, len(frames) // 4, len(frames) // 2, len(frames) - 1):
        scene.frame_set(int(frames[frame_index]))
        for j in (0, len(cubes) // 2, len(cubes) - 1):
            error = (
                cubes[j].matrix_world.translation - Vector(samples[frame_index, j, :3])
            ).length
            assert error < 1e-4, error
            probes.append(
                {
                    "frame": int(frames[frame_index]),
                    "name": cubes[j].name,
                    "position_blender": list(samples[frame_index, j, :3].astype(float)),
                    "quaternion_wxyz": list(samples[frame_index, j, 3:].astype(float)),
                }
            )
    scene.frame_set(1)
    bpy.ops.wm.save_as_mainfile(
        filepath=str((args.output / "benchmark.blend").resolve()), compress=True
    )
    pose_probes = sample_pose_probes(scene, frames, panels, args.humans)
    scene.frame_set(1)
    manifest = {
        "version": 2,
        "mesh_pose_panels": len(panels),
        "pose_probes": pose_probes,
        "seed": 7349,
        "grid": args.grid,
        "cubes": len(cubes),
        "seconds": args.seconds,
        "fps": args.fps,
        "frames": len(frames),
        "parented_lights": 20,
        "particle_count_per_side": args.particles,
        "rigify": rig,
        "physics_mean_drop": float(np.mean(samples[0, :, 2] - samples[-1, :, 2])),
        "build_seconds": time.perf_counter() - started,
        "probes": probes,
    }
    (args.output / "fixture.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(
        "STRESS complete",
        json.dumps(
            {k: v for k, v in manifest.items() if k not in {"probes", "pose_probes"}}
        ),
        flush=True,
    )


if __name__ == "__main__":
    main()
