"""Blender preferences, operators and panel for scene sync."""

import webbrowser

import bpy

from . import lifecycle


class IPPPreferences(bpy.types.AddonPreferences):
    bl_idname = __package__

    port: bpy.props.IntProperty(
        name="Local HTTPS port", default=8118, min=1024, max=65535
    )
    viewer_url: bpy.props.StringProperty(
        name="Viewer URL", default="http://127.0.0.1:5178"
    )
    allowed_origins: bpy.props.StringProperty(
        name="Additional viewer origins", description="Comma-separated exact origins"
    )
    certificate: bpy.props.StringProperty(name="Certificate", subtype="FILE_PATH")
    private_key: bpy.props.StringProperty(name="Private key", subtype="FILE_PATH")
    auto_sync: bpy.props.BoolProperty(name="Sync edits automatically", default=True)

    def draw(self, context):
        for name in (
            "viewer_url",
            "port",
            "allowed_origins",
            "certificate",
            "private_key",
            "auto_sync",
        ):
            self.layout.prop(self, name)
        self.layout.label(text="Restart sync after changing connection settings.")


class IPPStart(bpy.types.Operator):
    bl_idname = "ipp.start"
    bl_label = "Start IPP Sync"

    def execute(self, context):
        try:
            lifecycle.start()
        except Exception as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class IPPStop(bpy.types.Operator):
    bl_idname = "ipp.stop"
    bl_label = "Stop IPP Sync"

    def execute(self, context):
        lifecycle.stop()
        return {"FINISHED"}


class IPPSync(bpy.types.Operator):
    bl_idname = "ipp.sync"
    bl_label = "Sync Scene"

    def execute(self, context):
        try:
            lifecycle.start().sync()
        except Exception as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class IPPOpenViewer(bpy.types.Operator):
    bl_idname = "ipp.open_viewer"
    bl_label = "Open Viewer"

    def execute(self, context):
        try:
            webbrowser.open(lifecycle.start().onboarding_url)
        except Exception as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        return {"FINISHED"}


class IPPPanel(bpy.types.Panel):
    bl_label = "IPP Scene Sync"
    bl_idname = "IPP_PT_scene_sync"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "IPP"

    def draw(self, context):
        layout = self.layout
        server = lifecycle.get_server()
        if server is None:
            layout.operator("ipp.start")
        else:
            layout.label(text=f"Connected endpoint: {server.origin}")
            layout.label(text=f"Export revision {server.revision}")
            layout.operator("ipp.sync")
            layout.operator("ipp.stop")
        layout.operator("ipp.open_viewer")
        if lifecycle.get_error():
            layout.label(text=lifecycle.get_error(), icon="ERROR")


classes = (IPPPreferences, IPPStart, IPPStop, IPPSync, IPPOpenViewer, IPPPanel)
