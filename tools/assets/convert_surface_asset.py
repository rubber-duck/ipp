#!/usr/bin/env python3
"""Convert static TrueType fonts and a deliberately limited SVG subset for IPP."""

from __future__ import annotations

import argparse
import math
import re
import struct
import xml.etree.ElementTree as ET
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path

import pathops
from fontTools.pens.basePen import BasePen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.svgLib.path import parse_path
from fontTools.svgLib.path.arc import EllipticalArc
from fontTools.ttLib import TTFont


class ConversionError(ValueError):
    """An unsupported or malformed source with source-element context."""


Point = tuple[float, float]
Segment = tuple[int, tuple[float, ...]]


@dataclass
class Contour:
    start: Point
    segments: list[Segment] = field(default_factory=list)


class QuadraticPen(BasePen):
    def __init__(self, glyph_set=None, *, ignore_empty_contours: bool = False) -> None:
        super().__init__(glyph_set)
        self.contours: list[Contour] = []
        self.current: Contour | None = None
        self.ignore_empty_contours = ignore_empty_contours

    def _moveTo(self, point: Point) -> None:
        if self.current is not None:
            raise ConversionError("open contour before move")
        self.current = Contour(tuple(map(float, point)))

    def _lineTo(self, point: Point) -> None:
        assert self.current is not None
        self.current.segments.append((0, tuple(map(float, point))))

    def _qCurveToOne(self, control: Point, point: Point) -> None:
        assert self.current is not None
        self.current.segments.append((1, (*map(float, control), *map(float, point))))

    def _curveToOne(self, _a: Point, _b: Point, _c: Point) -> None:
        raise ConversionError("cubic reached quadratic output")

    def _closePath(self) -> None:
        self._finish(True)

    def _endPath(self) -> None:
        # SVG fill closes open subpaths implicitly. Font glyf contours are closed.
        self._finish(True)

    def _finish(self, closed: bool) -> None:
        assert self.current is not None
        if not closed:
            raise ConversionError("open paths cannot form a filled contour")
        if not self.current.segments:
            if self.ignore_empty_contours:
                self.current = None
                return
            raise ConversionError("empty contour")
        self.contours.append(self.current)
        self.current = None


def _pack_contours(contours: Iterable[Contour]) -> bytes:
    output = bytearray()
    for contour in contours:
        output.extend(struct.pack("<2fI", *contour.start, len(contour.segments)))
        for kind, values in contour.segments:
            output.extend(struct.pack("<B3x", kind))
            output.extend(struct.pack(f"<{len(values)}f", *values))
    return bytes(output)


def _bounds(contours: Iterable[Contour]) -> tuple[float, float, float, float]:
    points: list[Point] = []
    for contour in contours:
        points.append(contour.start)
        for kind, values in contour.segments:
            if kind == 1:
                points.append((values[0], values[1]))
            points.append((values[-2], values[-1]))
    if not points:
        return (0.0, 0.0, 0.0, 0.0)
    return (
        min(point[0] for point in points),
        min(point[1] for point in points),
        max(point[0] for point in points),
        max(point[1] for point in points),
    )


def _kern_pairs(
    font: TTFont, glyph_ids: dict[str, int]
) -> dict[tuple[int, int], float]:
    result: dict[tuple[int, int], float] = {}
    if "kern" in font:
        for table in font["kern"].kernTables:
            if (
                getattr(table, "format", None) == 0
                and getattr(table, "coverage", 1) & 1
            ):
                for (left, right), value in table.kernTable.items():
                    result[(glyph_ids[left], glyph_ids[right])] = float(value)
    if "GPOS" not in font:
        return result
    gpos = font["GPOS"].table
    if gpos.LookupList is None or gpos.FeatureList is None:
        return result
    feature_indices: set[int] = set()
    if gpos.ScriptList is not None:
        preferred = [
            record
            for record in gpos.ScriptList.ScriptRecord
            if record.ScriptTag in {"DFLT", "latn"}
        ]
        for record in preferred:
            language = record.Script.DefaultLangSys
            if language is not None:
                feature_indices.update(language.FeatureIndex)
                if language.ReqFeatureIndex != 0xFFFF:
                    feature_indices.add(language.ReqFeatureIndex)
    lookup_indices: list[int] = []
    for index in sorted(feature_indices):
        feature = gpos.FeatureList.FeatureRecord[index]
        if feature.FeatureTag == "kern":
            lookup_indices.extend(feature.Feature.LookupListIndex)
    gpos_pairs: dict[tuple[int, int], float] = {}
    for lookup_index in dict.fromkeys(lookup_indices):
        lookup = gpos.LookupList.Lookup[lookup_index]
        subtables = []
        if lookup.LookupType == 2:
            subtables = lookup.SubTable
        elif lookup.LookupType == 9:
            subtables = [
                extension.ExtSubTable
                for extension in lookup.SubTable
                if extension.ExtensionLookupType == 2
            ]
        for subtable in subtables:
            coverage = subtable.Coverage.glyphs
            if subtable.Format == 1:
                for left, pair_set in zip(coverage, subtable.PairSet, strict=True):
                    for record in pair_set.PairValueRecord:
                        value = getattr(record.Value1, "XAdvance", 0) or 0
                        if value:
                            key = (glyph_ids[left], glyph_ids[record.SecondGlyph])
                            gpos_pairs[key] = gpos_pairs.get(key, 0.0) + float(value)
            elif subtable.Format == 2:
                left_classes = subtable.ClassDef1.classDefs
                right_classes = subtable.ClassDef2.classDefs
                all_names = font.getGlyphOrder()
                for left in coverage:
                    class1 = left_classes.get(left, 0)
                    for right in all_names:
                        class2 = right_classes.get(right, 0)
                        value = (
                            getattr(
                                subtable.Class1Record[class1]
                                .Class2Record[class2]
                                .Value1,
                                "XAdvance",
                                0,
                            )
                            or 0
                        )
                        if value:
                            key = (glyph_ids[left], glyph_ids[right])
                            gpos_pairs[key] = gpos_pairs.get(key, 0.0) + float(value)
    return gpos_pairs or result


def convert_font(source: Path, destination: Path) -> None:
    font = TTFont(source, lazy=False)
    if "fvar" in font:
        raise ConversionError("variable fonts must be pre-instanced")
    if "glyf" not in font:
        raise ConversionError("font must use TrueType glyf outlines")
    order = font.getGlyphOrder()
    if not order or order[0] != ".notdef":
        raise ConversionError("glyph 0 must be .notdef")
    glyph_ids = {name: index for index, name in enumerate(order)}
    glyph_set = font.getGlyphSet()
    glyph_records = bytearray()
    for name in order:
        # Patched icon fonts can contain single-point TrueType contours used as
        # metadata anchors. They enclose no area and therefore have no Surface
        # geometry to preserve.
        pen = QuadraticPen(glyph_set, ignore_empty_contours=True)
        glyph_set[name].draw(pen)
        advance, lsb = font["hmtx"].metrics[name]
        bounds = _bounds(pen.contours)
        glyph_records.extend(
            struct.pack("<6fI", float(advance), float(lsb), *bounds, len(pen.contours))
        )
        glyph_records.extend(_pack_contours(pen.contours))
    cmap = sorted(
        (codepoint, glyph_ids[name])
        for codepoint, name in font.getBestCmap().items()
        if name in glyph_ids
    )
    kern = sorted(_kern_pairs(font, glyph_ids).items())
    hhea = font["hhea"]
    output = bytearray(b"IPPF")
    output.extend(
        struct.pack(
            "<II3f3I",
            1,
            font["head"].unitsPerEm,
            float(hhea.ascent),
            float(hhea.descent),
            float(hhea.lineGap),
            len(order),
            len(cmap),
            len(kern),
        )
    )
    output.extend(glyph_records)
    for codepoint, glyph_id in cmap:
        output.extend(struct.pack("<II", codepoint, glyph_id))
    for (left, right), value in kern:
        output.extend(struct.pack("<IIf", left, right, value))
    destination.write_bytes(output)


def _number(value: str, label: str) -> float:
    match = re.fullmatch(
        r"\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)\s*", value
    )
    if not match:
        raise ConversionError(f"{label}: only unitless SVG numbers are supported")
    result = float(match.group(1))
    if not math.isfinite(result):
        raise ConversionError(f"{label}: non-finite number")
    return result


def _color(value: str) -> tuple[int, int, int]:
    value = value.strip().lower()
    names = {
        "black": (0, 0, 0),
        "white": (255, 255, 255),
        "red": (255, 0, 0),
        "green": (0, 128, 0),
        "blue": (0, 0, 255),
    }
    if value in names:
        return names[value]
    if re.fullmatch(r"#[0-9a-f]{6}", value):
        return tuple(int(value[index : index + 2], 16) for index in (1, 3, 5))  # type: ignore[return-value]
    if re.fullmatch(r"#[0-9a-f]{3}", value):
        return tuple(int(value[index] * 2, 16) for index in (1, 2, 3))  # type: ignore[return-value]
    raise ConversionError(f"unsupported solid color {value!r}")


def _style(element: ET.Element, inherited: dict[str, str]) -> dict[str, str]:
    tag = element.tag.rsplit("}", 1)[-1]
    supported = {
        "fill",
        "stroke",
        "fill-rule",
        "fill-opacity",
        "stroke-opacity",
        "stroke-width",
        "stroke-linecap",
        "stroke-linejoin",
        "stroke-miterlimit",
        "stroke-dasharray",
        "stroke-dashoffset",
    }
    style = dict(inherited)
    for key in supported:
        if key in element.attrib:
            style[key] = element.attrib[key]
    for part in element.attrib.get("style", "").split(";"):
        if part.strip():
            key, separator, value = part.partition(":")
            if not separator:
                raise ConversionError(f"{tag}: malformed style")
            key = key.strip()
            if key not in supported:
                raise ConversionError(f"{tag}: unsupported style property {key!r}")
            style[key] = value.strip()
    if "opacity" in element.attrib or "opacity" in style:
        raise ConversionError(
            f"{tag}: element/group opacity is unsupported; use fill-opacity or stroke-opacity"
        )
    return style


def _shape_path(element: ET.Element) -> str | None:
    tag = element.tag.rsplit("}", 1)[-1]
    a = element.attrib
    n = lambda key, default="0": _number(a.get(key, default), f"{tag}.{key}")
    if tag == "path":
        return a.get("d", "")
    if tag == "line":
        return f"M {n('x1')} {n('y1')} L {n('x2')} {n('y2')}"
    if tag in ("polyline", "polygon"):
        points = a.get("points", "")
        number_pattern = re.compile(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?")
        values = []
        end = 0
        for match in number_pattern.finditer(points):
            if points[end : match.start()].strip(" ,\t\r\n"):
                raise ConversionError(f"{tag}: malformed points")
            values.append(float(match.group()))
            end = match.end()
        if points[end:].strip(" ,\t\r\n"):
            raise ConversionError(f"{tag}: malformed points")
        if len(values) < 4 or len(values) % 2:
            raise ConversionError(f"{tag}: malformed points")
        path = "M " + " L ".join(
            f"{values[i]} {values[i + 1]}" for i in range(0, len(values), 2)
        )
        return path + (" Z" if tag == "polygon" else "")
    if tag == "rect":
        x, y, width, height = n("x"), n("y"), n("width"), n("height")
        rx, ry = n("rx"), n("ry")
        if width <= 0 or height <= 0 or rx < 0 or ry < 0:
            raise ConversionError("rect: dimensions must be positive")
        if rx == 0 and ry == 0:
            return f"M{x} {y}h{width}v{height}h{-width}z"
        if rx == 0:
            rx = ry
        if ry == 0:
            ry = rx
        rx, ry = min(rx, width / 2), min(ry, height / 2)
        return f"M{x + rx} {y}H{x + width - rx}A{rx} {ry} 0 0 1 {x + width} {y + ry}V{y + height - ry}A{rx} {ry} 0 0 1 {x + width - rx} {y + height}H{x + rx}A{rx} {ry} 0 0 1 {x} {y + height - ry}V{y + ry}A{rx} {ry} 0 0 1 {x + rx} {y}z"
    if tag in ("circle", "ellipse"):
        cx, cy = n("cx"), n("cy")
        rx, ry = (n("r"), n("r")) if tag == "circle" else (n("rx"), n("ry"))
        if rx <= 0 or ry <= 0:
            raise ConversionError(f"{tag}: radii must be positive")
        return f"M{cx - rx} {cy}A{rx} {ry} 0 1 0 {cx + rx} {cy}A{rx} {ry} 0 1 0 {cx - rx} {cy}z"
    return None


def _matrix(value: str | None) -> tuple[float, float, float, float, float, float]:
    result = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    if not value:
        return result
    token = re.compile(r"([A-Za-z]+)\s*\(([^()]*)\)")
    number = re.compile(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?")
    end = 0
    matched = False
    for match in token.finditer(value):
        if value[end : match.start()].strip(" ,\t\r\n"):
            raise ConversionError(f"malformed transform {value!r}")
        matched = True
        name, args_text = match.groups()
        args = []
        args_end = 0
        for number_match in number.finditer(args_text):
            if args_text[args_end : number_match.start()].strip(" ,\t\r\n"):
                raise ConversionError(f"malformed transform {name}({args_text})")
            parsed = float(number_match.group())
            if not math.isfinite(parsed):
                raise ConversionError(f"non-finite transform {name}({args_text})")
            args.append(parsed)
            args_end = number_match.end()
        if args_text[args_end:].strip(" ,\t\r\n"):
            raise ConversionError(f"malformed transform {name}({args_text})")
        if name == "matrix" and len(args) == 6:
            current = tuple(args)
        elif name == "translate" and len(args) in (1, 2):
            current = (1, 0, 0, 1, args[0], args[1] if len(args) == 2 else 0)
        elif name == "scale" and len(args) in (1, 2):
            current = (args[0], 0, 0, args[-1], 0, 0)
        elif name == "rotate" and len(args) in (1, 3):
            angle = math.radians(args[0])
            c, s = math.cos(angle), math.sin(angle)
            current = (c, s, -s, c, 0, 0)
            if len(args) == 3:
                current = _multiply(
                    _multiply((1, 0, 0, 1, args[1], args[2]), current),
                    (1, 0, 0, 1, -args[1], -args[2]),
                )
        elif name == "skewX" and len(args) == 1:
            current = (1, 0, math.tan(math.radians(args[0])), 1, 0, 0)
        elif name == "skewY" and len(args) == 1:
            current = (1, math.tan(math.radians(args[0])), 0, 1, 0, 0)
        else:
            raise ConversionError(f"unsupported transform {name}({args_text})")
        result = _multiply(result, current)  # type: ignore[arg-type]
        end = match.end()
    if not matched or value[end:].strip(" ,\t\r\n"):
        raise ConversionError(f"malformed transform {value!r}")
    return result


def _multiply(a, b):
    return (
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    )


def _linear_scale(transform) -> float:
    a, b, c, d = transform[:4]
    trace = a * a + b * b + c * c + d * d
    determinant = (a * d - b * c) ** 2
    largest = (trace + math.sqrt(max(0.0, trace * trace - 4 * determinant))) / 2
    return math.sqrt(max(0.0, largest))


def _bounded_arc_class(tolerance: float):
    class BoundedArc(EllipticalArc):
        def _decompose_to_cubic_curves(self):
            if self.center_point is None and not self._parametrize():
                return
            radius = max(abs(self.rx), abs(self.ry))
            if radius == 0:
                return
            ratio = min(1.0, tolerance / radius)
            step = 2 * math.acos(max(-1.0, 1.0 - ratio))
            segments = max(1, math.ceil(abs(self.theta_arc) / min(math.pi / 2, step)))
            original_arc = self.theta_arc
            self.theta_arc = original_arc / segments
            original_theta2 = self.theta2
            self.theta2 = self.theta1 + self.theta_arc
            for _ in range(segments):
                yield from super()._decompose_to_cubic_curves()
                self.theta1 = self.theta2
                self.theta2 += self.theta_arc
            self.theta_arc = original_arc
            self.theta2 = original_theta2

    return BoundedArc


def _path_contours(path_data: str, transform, tolerance: float) -> list[Contour]:
    local_tolerance = tolerance / max(_linear_scale(transform), 1e-12)
    pen = QuadraticPen()
    parse_path(
        path_data,
        TransformPen(
            Cu2QuPen(pen, local_tolerance / 2, reverse_direction=False), transform
        ),
        arc_class=_bounded_arc_class(local_tolerance / 2),
    )
    return pen.contours


def _stroke_contours(
    path_data: str,
    transform,
    tolerance: float,
    width: float,
    cap: str,
    join: str,
    miter_limit: float,
    dash_array: list[float] | None,
    dash_offset: float,
) -> list[Contour]:
    local_tolerance = tolerance / max(_linear_scale(transform), 1e-12)
    path = pathops.Path()
    parse_path(path_data, path.getPen())
    caps = {
        "butt": pathops.LineCap.BUTT_CAP,
        "round": pathops.LineCap.ROUND_CAP,
        "square": pathops.LineCap.SQUARE_CAP,
    }
    joins = {
        "miter": pathops.LineJoin.MITER_JOIN,
        "round": pathops.LineJoin.ROUND_JOIN,
        "bevel": pathops.LineJoin.BEVEL_JOIN,
    }
    if cap not in caps:
        raise ConversionError(f"unsupported stroke-linecap {cap!r}")
    if join not in joins:
        raise ConversionError(f"unsupported stroke-linejoin {join!r}")
    path.stroke(width, caps[cap], joins[join], miter_limit, dash_array, dash_offset)
    path.convertConicsToQuads(local_tolerance / 2)
    pen = QuadraticPen()
    path.draw(
        TransformPen(
            Cu2QuPen(pen, local_tolerance / 2, reverse_direction=False), transform
        )
    )
    return pen.contours


def convert_svg(source: Path, destination: Path, tolerance: float) -> None:
    if not math.isfinite(tolerance) or tolerance <= 0:
        raise ConversionError("quadratic tolerance must be finite and positive")
    root = ET.parse(source).getroot()
    if root.tag.rsplit("}", 1)[-1] != "svg":
        raise ConversionError("root: expected svg")
    view = [
        _number(value, "svg.viewBox")
        for value in re.split(r"[ ,]+", root.attrib.get("viewBox", ""))
        if value
    ]
    if len(view) != 4 or view[2] <= 0 or view[3] <= 0:
        raise ConversionError("svg.viewBox: expected x y width height")
    view_box = (view[0], view[1], view[0] + view[2], view[1] + view[3])
    layers: list[tuple[bytes, int, list[Contour]]] = []
    forbidden = {
        "linearGradient",
        "radialGradient",
        "mask",
        "clipPath",
        "filter",
        "image",
        "text",
        "use",
        "pattern",
        "foreignObject",
    }

    def visit(
        element: ET.Element,
        inherited: dict[str, str],
        parent_transform,
        *,
        paint: bool,
        root_element: bool = False,
    ) -> None:
        tag = element.tag.rsplit("}", 1)[-1]
        if tag == "svg" and not root_element:
            raise ConversionError("svg: nested viewports are unsupported")
        if tag in forbidden:
            raise ConversionError(f"{tag}: unsupported visual element")
        for attribute in (
            "clip-path",
            "mask",
            "filter",
            "display",
            "visibility",
            "vector-effect",
            "paint-order",
            "class",
        ):
            if attribute in element.attrib:
                raise ConversionError(f"{tag}: unsupported {attribute}")
        if tag not in {
            "svg",
            "g",
            "path",
            "rect",
            "circle",
            "ellipse",
            "line",
            "polyline",
            "polygon",
            "title",
            "desc",
            "metadata",
            "defs",
        }:
            raise ConversionError(f"{tag}: unsupported element")
        generic_attributes = {
            "id",
            "style",
            "transform",
            "fill",
            "stroke",
            "fill-rule",
            "fill-opacity",
            "stroke-opacity",
            "stroke-width",
            "stroke-linecap",
            "stroke-linejoin",
            "stroke-miterlimit",
            "stroke-dasharray",
            "stroke-dashoffset",
        }
        shape_attributes = {
            "svg": {"viewBox"},
            "g": set(),
            "defs": set(),
            "path": {"d"},
            "rect": {"x", "y", "width", "height", "rx", "ry"},
            "circle": {"cx", "cy", "r"},
            "ellipse": {"cx", "cy", "rx", "ry"},
            "line": {"x1", "y1", "x2", "y2"},
            "polyline": {"points"},
            "polygon": {"points"},
            "title": set(),
            "desc": set(),
            "metadata": set(),
        }
        for attribute in element.attrib:
            if attribute not in generic_attributes | shape_attributes[tag]:
                raise ConversionError(f"{tag}: unsupported attribute {attribute!r}")
        if tag in {"title", "desc", "metadata"}:
            return
        style = _style(element, inherited)
        transform = _multiply(
            parent_transform, _matrix(element.attrib.get("transform"))
        )
        path_data = _shape_path(element)
        if path_data and paint:
            final_transform = transform
            contours = _path_contours(path_data, final_transform, tolerance)
            fill = style.get("fill", "black")
            if fill != "none" and contours:
                fill_rule = style.get("fill-rule", "nonzero")
                if fill_rule not in ("nonzero", "evenodd"):
                    raise ConversionError(f"{tag}.fill-rule: unsupported {fill_rule!r}")
                opacity = _number(style.get("fill-opacity", "1"), f"{tag}.fill-opacity")
                if not 0 <= opacity <= 1:
                    raise ConversionError(f"{tag}.fill-opacity: outside 0..1")
                layers.append(
                    (
                        bytes((*_color(fill), round(opacity * 255))),
                        1 if fill_rule == "evenodd" else 0,
                        contours,
                    )
                )
            if style.get("stroke", "none") != "none":
                width = _number(style.get("stroke-width", "1"), f"{tag}.stroke-width")
                if width <= 0:
                    raise ConversionError(f"{tag}.stroke-width: must be positive")
                miter_limit = _number(
                    style.get("stroke-miterlimit", "4"), f"{tag}.stroke-miterlimit"
                )
                dash_text = style.get("stroke-dasharray", "none")
                dash_array = None
                if dash_text != "none":
                    dash_array = [
                        _number(value, f"{tag}.stroke-dasharray")
                        for value in re.split(r"[ ,]+", dash_text)
                        if value
                    ]
                    if not dash_array or any(value <= 0 for value in dash_array):
                        raise ConversionError(
                            f"{tag}.stroke-dasharray: values must be positive"
                        )
                    if len(dash_array) % 2:
                        dash_array *= 2
                stroke_contours = _stroke_contours(
                    path_data,
                    final_transform,
                    tolerance,
                    width,
                    style.get("stroke-linecap", "butt"),
                    style.get("stroke-linejoin", "miter"),
                    miter_limit,
                    dash_array,
                    _number(
                        style.get("stroke-dashoffset", "0"), f"{tag}.stroke-dashoffset"
                    ),
                )
                opacity = _number(
                    style.get("stroke-opacity", "1"), f"{tag}.stroke-opacity"
                )
                if not 0 <= opacity <= 1:
                    raise ConversionError(f"{tag}.stroke-opacity: outside 0..1")
                layers.append(
                    (
                        bytes((*_color(style["stroke"]), round(opacity * 255))),
                        0,
                        stroke_contours,
                    )
                )
        child_paint = paint and tag != "defs"
        for child in element:
            visit(child, style, transform, paint=child_paint)

    visit(root, {}, (1, 0, 0, 1, 0, 0), paint=True, root_element=True)
    all_contours = [contour for _, _, contours in layers for contour in contours]
    output = bytearray(b"IPPD")
    output.extend(
        struct.pack(
            "<I4f4ffI", 1, *view_box, *_bounds(all_contours), tolerance, len(layers)
        )
    )
    for color, rule, contours in layers:
        output.extend(color)
        output.extend(struct.pack("<B3xI", rule, len(contours)))
        output.extend(_pack_contours(contours))
    destination.write_bytes(output)


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="kind", required=True)
    font = subparsers.add_parser("font")
    font.add_argument("source", type=Path)
    font.add_argument("destination", type=Path)
    svg = subparsers.add_parser("svg")
    svg.add_argument("source", type=Path)
    svg.add_argument("destination", type=Path)
    svg.add_argument("--tolerance", type=float, default=0.05)
    args = parser.parse_args()
    if args.kind == "font":
        convert_font(args.source, args.destination)
    else:
        convert_svg(args.source, args.destination, args.tolerance)


if __name__ == "__main__":
    main()
