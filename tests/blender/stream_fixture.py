"""Real Blender import exceeding one 4,096-command frame without extra draws."""

import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).parent))
import capacity_fixture
import fixture


def create_scene():
    capacity_fixture.create_scene()
    for index in range(2100):
        obj = bpy.data.objects.new(f"stream-empty-{index}", None)
        fixture.identify(obj, obj.name)
        bpy.context.scene.collection.objects.link(obj)


def apply_command(command):
    fixture.apply_command(command)
