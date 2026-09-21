"""Real scene exceeding former object, image, mesh-byte and sample quotas."""

import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).parent))
import fixture


def create_scene():
    fixture.create_scene()
    scene = bpy.context.scene
    cube = bpy.data.objects["fixture-cube"]
    cube.animation_data_clear()
    cube.scale = (0.15, 0.15, 0.15)
    for index in range(300):
        copy = cube.copy()
        fixture.identify(copy, f"capacity-cube-{index:03}")
        copy.location = ((index % 20 - 9.5) * 0.18, 0.7, index // 20 * 0.18)
        scene.collection.objects.link(copy)
    panel = bpy.data.objects["fixture-panel"]
    image = next(
        node.image
        for node in panel.data.materials[0].node_tree.nodes
        if node.type == "TEX_IMAGE"
    )
    image.scale(1024, 1536)
    image.pack()
    # Preserve a long action at its authored cadence, including a key after the
    # old 120-sample ceiling. The browser independently seeks this later pose.
    rig = bpy.data.objects["fixture-rig"]
    pose = rig.pose.bones["child"]
    pose.rotation_euler.z = -0.8
    pose.keyframe_insert("rotation_euler", index=2, frame=241)
    scene.frame_end = 241
    # One dense rigid mesh exceeds the old 1 MiB payload budget while respecting
    # the actual u16 mesh-index format. Place it behind the character panel.
    bpy.ops.mesh.primitive_grid_add(x_subdivisions=104, y_subdivisions=104, size=1)
    grid = fixture.identify(bpy.context.object, "capacity-dense-grid")
    grid.location = (0, 2, -0.2)
    grid.data.materials.append(fixture.material("Capacity floor", (0.3, 0.3, 0.3)))
    scene.frame_set(1)
    bpy.context.view_layer.update()


def apply_command(command):
    fixture.apply_command(command)
