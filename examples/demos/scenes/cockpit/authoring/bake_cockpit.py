"""Bake immutable runtime lighting without changing the editable presentation.

Run after build_cockpit.py. The runtime scene uses only supported opaque
Principled inputs; the application replaces the atmosphere's placeholder.
"""

from pathlib import Path

import bpy

ROOT = Path(__file__).resolve().parent


def select(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj


def copy_evaluated(obj, scene, graph):
    mesh = bpy.data.meshes.new_from_object(obj.evaluated_get(graph), depsgraph=graph)
    new = bpy.data.objects.new(obj.name + "-runtime", mesh)
    new.matrix_world = obj.matrix_world.copy()
    new["ipp_id"] = obj.get("ipp_id", obj.name)
    scene.collection.objects.link(new)
    return new


def baked_material(name, image):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    material["ipp_unlit"] = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Roughness"].default_value = 1
    tex = material.node_tree.nodes.new("ShaderNodeTexImage")
    tex.image = image
    material.node_tree.links.new(tex.outputs["Color"], shader.inputs["Base Color"])
    return material


def bake_object(obj, name, width, height, unwrap=False):
    select(obj)
    if unwrap:
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.smart_project(angle_limit=1.15, island_margin=0.004)
        bpy.ops.object.mode_set(mode="OBJECT")
    image = bpy.data.images.new(name, width=width, height=height, alpha=False)
    for index, original in enumerate(list(obj.data.materials)):
        mat = original.copy()
        mat.name = "Bake source " + original.name
        obj.data.materials[index] = mat
        shader = mat.node_tree.nodes.get("Principled BSDF")
        if shader:
            shader.inputs["Metallic"].default_value = 0
            shader.inputs["Roughness"].default_value = 0.7
        target = mat.node_tree.nodes.new("ShaderNodeTexImage")
        target.image = image
        mat.node_tree.nodes.active = target
    bpy.ops.object.bake(type="COMBINED")
    image.filepath_raw = str(ROOT / (name + ".png"))
    image.file_format = "PNG"
    image.save()
    image.pack()
    image.filepath = "//" + name + ".png"
    obj.data.materials.clear()
    obj.data.materials.append(baked_material(name, image))
    for polygon in obj.data.polygons:
        polygon.material_index = 0
    return image


def split_interior(obj, scene):
    """Respect the runtime's u16 mesh-corner bound while retaining baked UVs."""
    mesh = obj.data
    mesh.calc_loop_triangles()
    triangles = list(mesh.loop_triangles)
    uv = mesh.uv_layers.active.data
    normals = [n.vector.copy() for n in mesh.corner_normals]
    parts = []
    for first in range(0, len(triangles), 18000):
        vertices, coords, corner_normals = [], [], []
        for triangle in triangles[first : first + 18000]:
            for loop in triangle.loops:
                vertices.append(mesh.vertices[mesh.loops[loop].vertex_index].co.copy())
                coords.append(uv[loop].uv.copy())
                corner_normals.append(normals[loop])
        name = "cockpit-interior-" + str(len(parts))
        data = bpy.data.meshes.new(name)
        data.from_pydata(
            vertices, [], [(i, i + 1, i + 2) for i in range(0, len(vertices), 3)]
        )
        data.update()
        layer = data.uv_layers.new(name="Baked light atlas")
        for dest, value in zip(layer.data, coords):
            dest.uv = value
        for face in data.polygons:
            face.use_smooth = True
        data.normals_split_custom_set(corner_normals)
        data.materials.append(mesh.materials[0])
        part = bpy.data.objects.new(name, data)
        part.matrix_world = obj.matrix_world.copy()
        part["ipp_id"] = name
        scene.collection.objects.link(part)
        parts.append(part)
    bpy.data.objects.remove(obj, do_unlink=True)
    return parts


def bake():
    presentation = bpy.data.scenes["Cockpit presentation"]
    bpy.context.window.scene = presentation
    graph = bpy.context.evaluated_depsgraph_get()
    old = bpy.data.scenes.get("Cockpit runtime")
    if old:
        for obj in list(old.objects):
            if len(obj.users_scene) == 1:
                bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.scenes.remove(old)
    runtime = bpy.data.scenes.new("Cockpit runtime")
    runtime.world = presentation.world
    copies = [
        copy_evaluated(o, runtime, graph)
        for o in presentation.objects
        if o.get("cockpit_static")
    ]
    planet = copy_evaluated(presentation.objects["cockpit-planet"], runtime, graph)
    planet.name = "cockpit-planet-runtime"
    backdrop = copy_evaluated(presentation.objects["cockpit-space"], runtime, graph)
    star_image = next(
        n.image
        for n in presentation.objects["cockpit-space"].data.materials[0].node_tree.nodes
        if n.type == "TEX_IMAGE"
    )
    backdrop.data.materials.clear()
    backdrop.data.materials.append(baked_material("Runtime stars", star_image))
    atmosphere = copy_evaluated(
        presentation.objects["cockpit-atmosphere"], runtime, graph
    )
    atmosphere.data.materials.clear()
    placeholder = bpy.data.materials.new("Atmosphere application shader placeholder")
    placeholder.diffuse_color = (0.04, 0.12, 0.22, 1)
    placeholder["ipp_unlit"] = True
    atmosphere.data.materials.append(placeholder)
    # Omit the opaque placeholder from extraction. The application reuses the
    # planet mesh for a slightly expanded transparent shader shell.
    atmosphere.hide_render = True
    atmosphere["runtime_material"] = "animated-atmosphere"
    atmosphere["ipp_id"] = "cockpit-atmosphere-preview-mesh"
    atmosphere_anchor = bpy.data.objects.new("cockpit-atmosphere-anchor", None)
    atmosphere_anchor["ipp_id"] = "cockpit-atmosphere"
    atmosphere_anchor.matrix_world = planet.matrix_world.copy()
    atmosphere_anchor.scale = (1.0076666667,) * 3
    runtime.collection.objects.link(atmosphere_anchor)
    for source in presentation.objects:
        if source.type in {"EMPTY", "CAMERA"}:
            obj = source.copy()
            if source.data:
                obj.data = source.data.copy()
            runtime.collection.objects.link(obj)
            obj.name = source.name + "-runtime"
            if source.type == "CAMERA":
                runtime.camera = obj
        elif source.type == "LIGHT":
            runtime.collection.objects.link(source)
    bpy.context.window.scene = runtime
    bpy.ops.object.select_all(action="DESELECT")
    for obj in copies:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = copies[0]
    bpy.ops.object.join()
    interior = bpy.context.object
    interior.name = "cockpit-interior"
    interior["ipp_id"] = "cockpit-interior"
    runtime.render.engine = "CYCLES"
    runtime.cycles.samples = 64
    runtime.cycles.max_bounces = 4
    runtime.render.bake.use_pass_glossy = False
    runtime.render.bake.use_pass_transmission = False
    runtime.render.bake.margin = 8
    # Planet and atmosphere are outside the ship. Avoid their huge UV area
    # diluting the dedicated bridge light atlas.
    lights = [obj for obj in runtime.objects if obj.type == "LIGHT"]
    links = [(obj, obj.light_linking.receiver_collection) for obj in lights]
    try:
        for obj in lights:
            obj.light_linking.receiver_collection = None
        bake_object(interior, "cockpit-baked-lighting", 2048, 2048, unwrap=True)
        bake_object(planet, "cockpit-planet-lit", 2048, 1024)
    finally:
        for obj, receiver in links:
            obj.light_linking.receiver_collection = receiver
    for obj in list(runtime.objects):
        if obj.type == "LIGHT":
            runtime.collection.objects.unlink(obj)
    runtime.world = bpy.data.worlds.new("Runtime baked ambient")
    runtime.world.use_nodes = True
    runtime.world.node_tree.nodes.get("Background").inputs["Strength"].default_value = 0
    runtime.render.resolution_x = presentation.render.resolution_x
    runtime.render.resolution_y = presentation.render.resolution_y
    runtime["export_note"] = (
        "Export only this scene. Interactive surfaces and atmosphere are supplied by the application."
    )
    parts = split_interior(interior, runtime)
    presentation["runtime_bake_pending"] = False
    runtime["source_shell"] = (
        "cockpit-console-shell: evaluated Boolean, chamfer and continuous sweep modifiers"
    )
    bpy.context.window.scene = presentation
    bpy.ops.outliner.orphans_purge(do_recursive=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT / "cockpit.blend"))
    return {
        "runtime_objects": len(runtime.objects),
        "interior_parts": len(parts),
        "interior_triangles": sum(len(p.data.polygons) for p in parts),
        "scene": runtime.name,
    }


if __name__ == "__main__":
    bake()
