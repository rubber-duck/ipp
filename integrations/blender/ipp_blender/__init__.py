"""Blender addon registration and public sync entry points."""

import bpy

from . import lifecycle, ui

start = lifecycle.start
stop = lifecycle.stop
get_server = lifecycle.get_server
tick = lifecycle.tick
preferences = lifecycle.preferences


def register():
    for cls in ui.classes:
        bpy.utils.register_class(cls)
    lifecycle.register_handlers()


def unregister():
    lifecycle.unregister_handlers()
    for cls in reversed(ui.classes):
        bpy.utils.unregister_class(cls)
