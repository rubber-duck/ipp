"""Main-thread server lifecycle, dirty handlers and timer state."""

import os
import time
from pathlib import Path

import bpy
from bpy.app.handlers import persistent

_server = None
_error = ""
_dirty_at = None
_restart_after_load = False
_message_owner = object()


def preferences():
    return bpy.context.preferences.addons[__package__].preferences


def start(**options):
    global _server, _error, _dirty_at
    from .exporter import export_scene
    from .server import SceneServer

    if _server is not None:
        return _server
    if not options:
        settings = preferences()
        options = {
            "port": settings.port,
            "certificate": settings.certificate
            or os.environ.get("IPP_BLENDER_CERTIFICATE", ""),
            "private_key": settings.private_key
            or os.environ.get("IPP_BLENDER_PRIVATE_KEY", ""),
            "viewer_url": settings.viewer_url,
            "allowed_origins": tuple(
                value.strip()
                for value in settings.allowed_origins.split(",")
                if value.strip()
            ),
        }
    options.setdefault(
        "directory", Path(bpy.utils.user_resource("CONFIG")) / "ipp-blender"
    )
    try:
        _server = SceneServer(export_scene, **options).start()
        _error = ""
        _dirty_at = None
    except Exception as error:
        _error = str(error)
        raise
    return _server


def stop():
    global _server, _dirty_at
    if _server is not None:
        _server.close()
        _server = None
    _dirty_at = None


def get_error():
    return _error


def get_server():
    """Main-thread inspection/control entry point for the optional MCP driver."""
    return _server


def tick():
    global _dirty_at, _error
    if _server is not None:
        _server.pump()
        if _dirty_at is not None and time.monotonic() - _dirty_at >= 0.25:
            _dirty_at = None
            try:
                _server.sync()
                _error = ""
            except Exception as error:
                _error = str(error)
                print(f"IPP sync failed: {_error}")
    return 0.01


@persistent
def changed(*_args):
    global _dirty_at
    if _server is not None and not _server.exporting:
        if preferences().auto_sync:
            _dirty_at = time.monotonic()


def subscribe_names():
    bpy.msgbus.clear_by_owner(_message_owner)
    bpy.msgbus.subscribe_rna(
        key=(bpy.types.Object, "name"),
        owner=_message_owner,
        args=(),
        notify=changed,
        options={"PERSISTENT"},
    )


@persistent
def before_load(*_args):
    global _restart_after_load
    _restart_after_load = _server is not None
    stop()


@persistent
def after_load(*_args):
    global _restart_after_load
    subscribe_names()
    if _restart_after_load:
        _restart_after_load = False
        try:
            start()
        except Exception as error:
            print(f"IPP server could not restart: {error}")


def register_handlers():
    bpy.app.handlers.depsgraph_update_post.append(changed)
    bpy.app.handlers.load_pre.append(before_load)
    bpy.app.handlers.load_post.append(after_load)
    subscribe_names()
    if not bpy.app.background:
        bpy.app.timers.register(tick, persistent=True)


def unregister_handlers():
    stop()
    bpy.msgbus.clear_by_owner(_message_owner)
    if bpy.app.timers.is_registered(tick):
        bpy.app.timers.unregister(tick)
    for handlers, callback in (
        (bpy.app.handlers.depsgraph_update_post, changed),
        (bpy.app.handlers.load_pre, before_load),
        (bpy.app.handlers.load_post, after_load),
    ):
        if callback in handlers:
            handlers.remove(callback)
