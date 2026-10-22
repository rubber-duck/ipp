"""Bake registered scalar light contributions and pixel-preserving square tiles.

Run through the pinned Blender Python. Numerical masks are source data, not
colored lighting photographs. Source RGB previews the grayscale contribution.
The exporter copies this weight to linear alpha for runtime sRGB sampling.
"""

import json
import runpy
from pathlib import Path

import bpy
import numpy as np

ROOT = Path(__file__).resolve().parent
SIDE = 1024
GUTTER = 2


def load(path):
    image = bpy.data.images.load(str(path), check_existing=False)
    width, height = image.size
    values = np.empty(width * height * 4, dtype=np.float32)
    image.pixels.foreach_get(values)
    return values.reshape(height, width, 4)[::-1].copy()


def save(path, pixels, data=False):
    height, width = pixels.shape[:2]
    image = bpy.data.images.new(path.stem, width, height, alpha=True, is_data=data)
    image.pixels.foreach_set(
        np.ascontiguousarray(pixels[::-1], dtype=np.float32).ravel()
    )
    image.filepath_raw = str(path)
    image.file_format = "PNG"
    image.save()
    bpy.data.images.remove(image)


def polygon_mask(shape, polygons):
    height, width = shape
    result = np.zeros(shape, dtype=np.float32)
    for polygon in polygons:
        xs, ys = zip(*polygon, strict=True)
        left, right = max(0, min(xs) - 1), min(width, max(xs) + 2)
        top, bottom = max(0, min(ys) - 1), min(height, max(ys) + 2)
        yy, xx = np.mgrid[top:bottom, left:right]
        coverage = np.zeros(xx.shape, dtype=np.float32)
        for dy, dx in [(0.25, 0.25), (0.25, 0.75), (0.75, 0.25), (0.75, 0.75)]:
            x, y = xx + dx, yy + dy
            inside = np.zeros(xx.shape, dtype=bool)
            for a, b in zip(polygon, polygon[1:] + polygon[:1], strict=True):
                if a[1] == b[1]:
                    continue
                cross = (a[0] - b[0]) * (y - b[1]) / (a[1] - b[1]) + b[0]
                inside ^= ((a[1] > y) != (b[1] > y)) & (x < cross)
            coverage += inside * 0.25
        result[top:bottom, left:right] = np.maximum(
            result[top:bottom, left:right], coverage
        )
    return result


def contribution(base, polygons, rings=()):
    mask = polygon_mask(base.shape[:2], polygons)
    yy, xx = np.mgrid[: mask.shape[0], : mask.shape[1]]
    for cx, cy, rx, ry, thickness, segmented in rings:
        x, y = (xx + 0.5 - cx) / rx, (yy + 0.5 - cy) / ry
        distance = np.abs(np.sqrt(x * x + y * y) - 1) * min(rx, ry)
        ring = np.clip(thickness * 0.5 + 0.5 - distance, 0, 1)
        if segmented:
            ring *= np.cos(np.arctan2(y, x) * 8) > -0.65
        mask = np.maximum(mask, ring)
    # Tight optical spill retains the source geometry; no second image can drift.
    axis = np.arange(-9, 10, dtype=np.float32)
    kernel = np.exp(-0.5 * (axis / 3) ** 2)
    kernel /= kernel.sum()
    spill = np.apply_along_axis(
        lambda row: np.convolve(row, kernel, mode="same"), 1, mask
    )
    spill = np.apply_along_axis(
        lambda row: np.convolve(row, kernel, mode="same"), 0, spill
    )
    weight = np.clip(mask * 0.88 + spill * 0.12, 0, 1)
    weight *= base[:, :, 3] >= 0.5
    result = np.repeat(weight[:, :, None], 4, axis=2)
    result[:, :, 3] = 1
    return result


def tiles(name, base, light, count):
    directory = ROOT / "lighting-tiles"
    directory.mkdir(exist_ok=True)
    height, width = base.shape[:2]
    output = []
    for index in range(count):
        left, right = index * width // count, (index + 1) * width // count
        inner_width = right - left
        if max(inner_width, height) + 2 * GUTTER > SIDE:
            raise ValueError(f"Source exceeds square tile capacity: {name}")
        paths = {}
        for role, source in [("base", base), ("contribution", light)]:
            # Include adjacent source texels as a guard band at internal splits.
            x = np.clip(np.arange(left - GUTTER, right + GUTTER), 0, width - 1)
            y = np.clip(np.arange(-GUTTER, height + GUTTER), 0, height - 1)
            square = np.zeros((SIDE, SIDE, 4), dtype=np.float32)
            square[: len(y), : len(x)] = source[y[:, None], x[None, :]]
            path = directory / f"{name}-{index}-{role}.png"
            save(path, square, data=role == "contribution")
            paths[role] = str(path.relative_to(ROOT))
        output.append(
            {
                **paths,
                "bounds": [left / width, 0, right / width, 1],
                "textureRect": [
                    GUTTER / SIDE,
                    GUTTER / SIDE,
                    (GUTTER + inner_width) / SIDE,
                    (GUTTER + height) / SIDE,
                ],
                "dimensions": [SIDE, SIDE],
                "sourcePixels": [left, 0, right, height],
            }
        )
    return output


def join_edges(first, second, axis, band=16):
    """Bake the old material's narrow join blend before subdividing its inputs.

    Resample only the neighboring edge along its length (the enhanced source
    quadrants differ by one or two pixels). Interior texels stay untouched.
    """
    a, b = np.moveaxis(first, axis, 0), np.moveaxis(second, axis, 0)
    a_edge, b_edge = a[-1].copy(), b[0].copy()

    def edge_at(edge, count):
        source = (np.arange(len(edge)) + 0.5) / len(edge)
        target = (np.arange(count) + 0.5) / count
        return np.stack(
            [np.interp(target, source, edge[:, channel]) for channel in range(4)],
            axis=1,
        )

    for distance in range(band):
        weight = 0.5 * (1 - distance / band) ** 2
        a[-1 - distance] = (
            a[-1 - distance] * (1 - weight) + edge_at(b_edge, len(a_edge)) * weight
        )
        b[distance] = b[distance] * (1 - weight) + edge_at(a_edge, len(b_edge)) * weight


def prepare():
    fixtures = runpy.run_path(str(ROOT / "lamp_fixtures.py"))
    result = {}
    bridge = {}
    for name, polygons in fixtures["BRIDGE"].items():
        base = load(ROOT / f"bridge-{name}-fixtures-base.png")
        light = contribution(base, polygons)
        # Filter color with coverage; hidden RGB in generated transparent windows
        # must not bleed onto the visible frame at a quadrant join.
        base[:, :, 3] = base[:, :, 3] >= 0.5
        base[:, :, :3] *= base[:, :, 3:4]
        bridge[name] = [base, light]
    for role in range(2):
        for row in ["upper", "lower"]:
            left, right = bridge[f"{row}-left"][role], bridge[f"{row}-right"][role]
            join_edges(left, right, 1)
            join_edges(right, left, 1)
        for side in ["left", "right"]:
            join_edges(bridge[f"upper-{side}"][role], bridge[f"lower-{side}"][role], 0)
    for name, (base, light) in bridge.items():
        np.divide(
            base[:, :, :3],
            base[:, :, 3:4],
            out=base[:, :, :3],
            where=base[:, :, 3:4] > 0,
        )
        save(ROOT / f"bridge-{name}-contribution.png", light, data=True)
        result[f"bridge-{name}"] = tiles(f"bridge-{name}", base, light, 2)
    base = load(ROOT / "console-matched-base.png")
    light = contribution(base, fixtures["CONSOLE"], fixtures["CONSOLE_RINGS"])
    save(ROOT / "console-contribution.png", light, data=True)
    result["console"] = tiles("console", base, light, 3)
    (ROOT / "lighting-tiles.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


if __name__ == "__main__":
    prepare()
