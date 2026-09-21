"""Shared coordinate basis and unsupported-feature error for Blender extraction."""

import math

from mathutils import Matrix

BASIS = Matrix.Rotation(-math.pi / 2, 4, "X")


class Unsupported(ValueError):
    """A source feature cannot be represented by the supported runtime subset."""
