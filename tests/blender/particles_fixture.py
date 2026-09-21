"""Native and baked effects for the real addon/HTTPS/WSS viewer environment."""

import bpy
from mathutils import Vector


def create_scene():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    scene = bpy.context.scene
    scene.frame_start = 1
    scene.frame_end = 49
    scene.render.fps = 24
    scene.use_gravity = False
    bpy.ops.mesh.primitive_plane_add(size=2)
    obj = bpy.context.object
    obj.name = "particle-source"
    obj["ipp_id"] = "particle-source"
    bpy.ops.object.particle_system_add()
    settings = obj.particle_systems[0].settings
    settings.count = 100
    settings.frame_start = 1
    settings.frame_end = 13
    settings.lifetime = 96
    settings.normal_factor = 1
    settings.render_type = "HALO"
    settings.particle_size = 0.12
    settings.use_rotations = False
    obj["ipp_particles"] = "NATIVE"
    camera = bpy.data.objects.new("camera", bpy.data.cameras.new("camera"))
    scene.collection.objects.link(camera)
    camera["ipp_id"] = "particle-camera"
    camera.location = (0, -7, 3)
    camera.rotation_euler = (
        (Vector((0, 0, 1)) - camera.location).to_track_quat("-Z", "Y").to_euler()
    )
    scene.camera = camera
    scene.frame_set(1)


def apply_command(command):
    if command["action"] == "bake":
        bpy.data.objects["particle-source"]["ipp_particles"] = "BAKED"
    else:
        raise ValueError("Unknown particle fixture command")
    bpy.context.view_layer.update()
