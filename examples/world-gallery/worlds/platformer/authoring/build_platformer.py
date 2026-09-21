"""Build the editable KayKit platformer source scene.

Run with Blender's factory startup so the user's open scene is never replaced:

    blender --background --factory-startup --python build_platformer.py -- \
      OUTPUT.blend PLATFORMER_PACK CHARACTER_ANIMATIONS_PACK PREVIEW.png ROUTE.json
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from route import route_document, route_points


def arguments() -> tuple[Path, Path, Path, Path, Path]:
    try:
        separator = sys.argv.index("--")
        output, platformer, animations, preview, route = sys.argv[separator + 1 :]
    except ValueError:
        raise SystemExit(
            "Expected OUTPUT.blend PLATFORMER_PACK CHARACTER_ANIMATIONS_PACK PREVIEW.png ROUTE.json"
        ) from None
    except IndexError:
        raise SystemExit(
            "Expected OUTPUT.blend PLATFORMER_PACK CHARACTER_ANIMATIONS_PACK PREVIEW.png ROUTE.json"
        ) from None
    return tuple(
        Path(value).resolve()
        for value in (output, platformer, animations, preview, route)
    )


OUTPUT, PLATFORMER_PACK, ANIMATIONS_PACK, PREVIEW, ROUTE = arguments()


def clear_scene() -> None:
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (
        bpy.data.actions,
        bpy.data.meshes,
        bpy.data.curves,
        bpy.data.materials,
    ):
        for datablock in list(datablocks):
            if datablock.users == 0:
                datablocks.remove(datablock)


def import_gltf(path: Path) -> list[bpy.types.Object]:
    before = set(bpy.data.objects)
    result = bpy.ops.import_scene.gltf(filepath=str(path))
    if "FINISHED" not in result:
        raise RuntimeError(f"Could not import {path}")
    return [obj for obj in bpy.data.objects if obj not in before]


def move_to_collection(obj: bpy.types.Object, collection: bpy.types.Collection) -> None:
    for owner in list(obj.users_collection):
        owner.objects.unlink(obj)
    collection.objects.link(obj)


def remove_root_motion(action: bpy.types.Action) -> None:
    """Hold the root bone location at its first sample; route motion owns travel."""
    for slot in action.slots:
        for layer in action.layers:
            for strip in layer.strips:
                try:
                    fcurves = strip.channelbag(slot).fcurves
                except RuntimeError:
                    continue
                for curve in fcurves:
                    if curve.data_path != 'pose.bones["root"].location':
                        continue
                    value = curve.evaluate(action.frame_range[0])
                    for point in curve.keyframe_points:
                        delta = value - point.co.y
                        point.co.y = value
                        point.handle_left.y += delta
                        point.handle_right.y += delta


def keep_locomotion_actions(rig: bpy.types.Object) -> None:
    selected = {
        "Walking_A": "Platformer_Walk",
        "Running_A": "Platformer_Run",
        "Crawling": "Platformer_Crawl",
    }
    actions = {}
    for source, target in selected.items():
        action = bpy.data.actions.get(source)
        if action is None:
            raise RuntimeError(f"Required KayKit action is missing: {source}")
        action.name = target
        remove_root_motion(action)
        actions[target] = action

    animation = rig.animation_data_create()
    # glTF imports one muted NLA track per source action. Replace that noisy
    # library with the three explicit bindings used by this scene.
    for track in list(animation.nla_tracks):
        animation.nla_tracks.remove(track)
    animation.action = actions["Platformer_Walk"]
    for name in ("Platformer_Run", "Platformer_Crawl"):
        track = animation.nla_tracks.new()
        track.name = f"Library {name}"
        track.mute = True
        action = actions[name]
        strip = track.strips.new(name, int(action.frame_range[0]), action)
        strip.action_frame_start, strip.action_frame_end = action.frame_range

    for action in list(bpy.data.actions):
        if action.name not in actions:
            bpy.data.actions.remove(action)


def import_character(
    character_collection: bpy.types.Collection, root: bpy.types.Object
):
    basic = ANIMATIONS_PACK / "Animations/gltf/Rig_Medium/Rig_Medium_MovementBasic.glb"
    advanced = (
        ANIMATIONS_PACK / "Animations/gltf/Rig_Medium/Rig_Medium_MovementAdvanced.glb"
    )
    basic_objects = import_gltf(basic)
    rig = next(obj for obj in basic_objects if obj.type == "ARMATURE")
    meshes = [
        obj
        for obj in basic_objects
        if obj.type == "MESH" and obj.name.startswith("Mannequin_")
    ]
    for obj in basic_objects:
        if obj != rig and obj not in meshes:
            bpy.data.objects.remove(obj, do_unlink=True)

    advanced_objects = import_gltf(advanced)
    for obj in advanced_objects:
        bpy.data.objects.remove(obj, do_unlink=True)

    if len(rig.data.bones) != 23:
        raise RuntimeError(
            f"Expected the 23-bone medium rig, found {len(rig.data.bones)}"
        )

    bpy.ops.object.select_all(action="DESELECT")
    for mesh in meshes:
        mesh.hide_set(False)
        mesh.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.join()
    character = bpy.context.view_layer.objects.active
    character.name = "platformer-character"
    character["ipp_id"] = "platformer-character"
    rig.name = "platformer-rig"
    rig["ipp_id"] = "platformer-rig"
    rig.parent = root
    rig.location = (0.0, 0.0, 0.0)

    for obj in (rig, character):
        move_to_collection(obj, character_collection)
    keep_locomotion_actions(rig)
    return rig, character


def import_piece(
    course: bpy.types.Collection,
    asset: str,
    colour: str,
    name: str,
    location: tuple[float, float, float],
    rotation_z: float = 0.0,
) -> bpy.types.Object:
    path = PLATFORMER_PACK / f"Assets/gltf/{colour}/{asset}_{colour}.gltf"
    objects = import_gltf(path)
    meshes = [obj for obj in objects if obj.type == "MESH"]
    if len(meshes) != 1:
        raise RuntimeError(f"Expected one mesh in {path}, found {len(meshes)}")
    piece = meshes[0]
    piece.name = name
    piece["ipp_id"] = "platformer-track-" + name.lower().replace(" ", "-")
    piece.location = location
    piece.rotation_euler[2] = rotation_z
    move_to_collection(piece, course)
    return piece


def build_course(course: bpy.types.Collection) -> None:
    # Six-unit tiles overlap at supported corners. The two slopes bridge the lower
    # and raised straights, leaving a continuous surface for every gait.
    placements = [
        ("platform_6x6x1", "blue", "Lower West", (-9, -9, 0), 0),
        ("platform_6x6x1", "green", "Lower Mid West", (-3, -9, 0), 0),
        ("platform_6x6x1", "yellow", "Lower Mid East", (3, -9, 0), 0),
        ("platform_6x6x1", "red", "Lower East", (9, -9, 0), 0),
        ("platform_6x6x1", "red", "East Approach", (9, -3, 0), 0),
        ("platform_slope_6x6x4", "yellow", "East Ramp", (9, 3, 0), 0),
        ("platform_6x6x1", "green", "Upper East", (9, 9, 3), 0),
        ("platform_6x6x1", "blue", "Upper Mid East", (3, 9, 3), 0),
        ("platform_6x6x1", "red", "Upper Mid West", (-3, 9, 3), 0),
        ("platform_6x6x1", "yellow", "Upper West", (-9, 9, 3), 0),
        ("platform_slope_6x6x4", "blue", "West Ramp", (-9, 3, 0), math.pi),
        ("platform_6x6x1", "green", "West Approach", (-9, -3, 0), 0),
    ]
    for asset, colour, name, location, rotation in placements:
        import_piece(course, asset, colour, name, location, rotation)

    # Small authored cues make the loop read as a platformer course while
    # keeping every route waypoint and the full centerline unobstructed.
    decorations = [
        ("flag_A", "blue", "Start Flag Outer", (-11.2, -10.8, 1.0), 0),
        ("flag_A", "yellow", "Start Flag Inner", (-11.2, -7.2, 1.0), math.pi),
        ("star", "yellow", "Lower Collectible", (0, -11.2, 2.0), 0),
        ("hoop", "red", "Upper Hoop", (3, 11.2, 4.0), 0),
        ("star", "blue", "Upper Collectible", (-3, 11.2, 5.0), 0),
    ]
    for asset, colour, name, location, rotation in decorations:
        import_piece(course, asset, colour, name, location, rotation)


def add_empty(name: str, identifier: str, parent: bpy.types.Object, location):
    obj = bpy.data.objects.new(name, None)
    obj["ipp_id"] = identifier
    obj.parent = parent
    obj.location = location
    bpy.context.scene.collection.objects.link(obj)
    return obj


def add_scene_objects(root: bpy.types.Object):
    target = add_empty(
        "platformer-camera-target", "platformer-camera-target", root, (0, 0, 1.35)
    )

    camera_data = bpy.data.cameras.new("Platformer Camera")
    camera_data.lens = 48
    camera = bpy.data.objects.new("platformer-camera", camera_data)
    camera["ipp_id"] = "platformer-camera"
    camera.parent = root
    camera.location = (14, 0, 6)
    bpy.context.scene.collection.objects.link(camera)

    direction = target.location - camera.location
    camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = camera

    light_data = bpy.data.lights.new("Platformer Overhead Light", "POINT")
    light_data.energy = 500
    light_data.color = (1.0, 0.79, 0.56)
    light_data["ipp_intensity"] = 3.5
    light_data["ipp_range"] = 12.0
    light = bpy.data.objects.new("platformer-overhead-light", light_data)
    light["ipp_id"] = "platformer-overhead-light"
    light.parent = root
    light.location = (0, 0, 4.8)
    bpy.context.scene.collection.objects.link(light)

    bpy.ops.mesh.primitive_ico_sphere_add(
        subdivisions=2, radius=0.45, location=(0, 0, 3.2)
    )
    orb = bpy.context.object
    orb.name = "platformer-orb"
    orb["ipp_id"] = "platformer-orb"
    orb.parent = root
    orb.location = (0, 0, 3.2)
    material = bpy.data.materials.new("Platformer Orb Preview")
    material.diffuse_color = (0.35, 0.82, 1.0, 1.0)
    material["ipp_unlit"] = True
    orb.data.materials.append(material)

    sun_data = bpy.data.lights.new("Platformer Sun", "SUN")
    sun_data.energy = 0.8
    sun_data.color = (1.0, 0.91, 0.78)
    sun_data["ipp_intensity"] = 0.8
    sun = bpy.data.objects.new("platformer-sun", sun_data)
    sun["ipp_id"] = "platformer-sun"
    sun.rotation_euler = (math.radians(28), math.radians(-22), math.radians(-35))
    bpy.context.scene.collection.objects.link(sun)
    return camera


def course_surface_height(
    course: bpy.types.Collection, x: float, y: float
) -> tuple[float, str] | None:
    depsgraph = bpy.context.evaluated_depsgraph_get()
    best = None
    for source in course.objects:
        if source.type != "MESH":
            continue
        obj = source.evaluated_get(depsgraph)
        inverse = obj.matrix_world.inverted()
        origin = inverse @ Vector((x, y, 20))
        direction = (inverse.to_3x3() @ Vector((0, 0, -1))).normalized()
        hit, location, _, _ = obj.ray_cast(origin, direction, distance=40)
        if hit:
            height = (obj.matrix_world @ location).z
            if best is None or height > best[0]:
                best = (height, obj.name)
    return best


def validate_route_support(course: bpy.types.Collection) -> dict:
    points = route_points()
    samples = []
    for index, start in enumerate(points):
        end = points[(index + 1) % len(points)]
        for step in range(17):
            amount = step / 16
            expected = start[2] + (end[2] - start[2]) * amount
            x = start[0] + (end[0] - start[0]) * amount
            y = start[1] + (end[1] - start[1]) * amount
            support = course_surface_height(course, x, y)
            if support is None:
                raise RuntimeError(f"Route has no support at ({x}, {y})")
            actual, name = support
            error = abs(actual - expected)
            samples.append((error, x, y, expected, actual, name))
    worst = max(samples)
    if worst[0] > 0.08:
        raise RuntimeError(
            "Route is above or below its support: "
            f"error={worst[0]:.4f} at ({worst[1]:.3f}, {worst[2]:.3f}), "
            f"expected={worst[3]:.4f}, actual={worst[4]:.4f}, object={worst[5]}"
        )
    return {
        "samples": len(samples),
        "maximumHeightError": round(worst[0], 6),
        "worstSupport": worst[5],
    }


def add_route_aid(authoring: bpy.types.Collection) -> None:
    curve = bpy.data.curves.new("Platformer Route (authoring aid)", "CURVE")
    curve.dimensions = "3D"
    curve.bevel_depth = 0.04
    spline = curve.splines.new("POLY")
    points = route_points()
    spline.points.add(len(points) - 1)
    for point, coordinate in zip(spline.points, points, strict=True):
        point.co = (*coordinate, 1.0)
    spline.use_cyclic_u = True
    obj = bpy.data.objects.new("Platformer Route (not exported)", curve)
    obj.hide_render = True
    authoring.objects.link(obj)


def add_preview_camera() -> bpy.types.Object:
    data = bpy.data.cameras.new("Overview Preview Camera")
    data.lens = 52
    camera = bpy.data.objects.new("Overview Preview Camera", data)
    camera.location = (26, -31, 25)
    camera.rotation_euler = (
        (Vector((0, 0, 2)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    )
    camera.hide_render = True
    bpy.context.scene.collection.objects.link(camera)
    return camera


def configure_scene() -> None:
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 960
    scene.render.resolution_y = 540
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.film_transparent = False
    scene.world.color = (0.14, 0.18, 0.25)
    scene.world.use_nodes = True
    background = scene.world.node_tree.nodes.get("Background")
    background.inputs["Color"].default_value = (0.14, 0.18, 0.25, 1.0)
    background.inputs["Strength"].default_value = 0.8
    scene.render.fps = 30
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0


def main() -> None:
    clear_scene()
    configure_scene()
    course = bpy.data.collections.new("Course")
    character_collection = bpy.data.collections.new("Character")
    authoring = bpy.data.collections.new("Authoring Aids (not exported)")
    for collection in (course, character_collection, authoring):
        bpy.context.scene.collection.children.link(collection)

    root = bpy.data.objects.new("platformer-root", None)
    root["ipp_id"] = "platformer-root"
    root.location = (-9.0, -9.0, 1.0)
    root.rotation_euler[2] = math.pi / 2
    bpy.context.scene.collection.objects.link(root)
    build_course(course)
    import_character(character_collection, root)
    side_camera = add_scene_objects(root)
    add_route_aid(authoring)
    overview = add_preview_camera()
    support = validate_route_support(course)

    # Save a readable authored walk pose rather than the imported rest pose.
    bpy.context.scene.frame_set(8)
    bpy.context.view_layer.update()

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    PREVIEW.parent.mkdir(parents=True, exist_ok=True)
    ROUTE.parent.mkdir(parents=True, exist_ok=True)
    ROUTE.write_text(json.dumps(route_document(), indent=2) + "\n")
    bpy.ops.file.pack_all()
    bpy.context.scene.camera = side_camera
    bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT))

    # Rendering from the overview camera does not change the saved authoring view.
    overview.hide_render = False
    bpy.context.scene.camera = overview
    bpy.context.scene.render.filepath = str(PREVIEW)
    bpy.ops.render.render(write_still=True)
    print(
        "PLATFORMER_BUILD",
        json.dumps(
            {
                "blend": str(OUTPUT),
                "preview": str(PREVIEW),
                "route": str(ROUTE),
                "bones": 23,
                "support": support,
                "actions": [
                    "Platformer_Walk",
                    "Platformer_Run",
                    "Platformer_Crawl",
                ],
            },
            sort_keys=True,
        ),
    )


main()
