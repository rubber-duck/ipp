"""Convert a bounded SVG animation subset into IPPD and an animation manifest."""

from __future__ import annotations

import argparse
import json
import math
import re
import tempfile
import xml.etree.ElementTree as ET
from copy import deepcopy
from itertools import pairwise
from pathlib import Path

try:
    from tools.convert_surface_asset import ConversionError, convert_svg
except ModuleNotFoundError:  # Direct execution from the tools directory.
    from convert_surface_asset import ConversionError, convert_svg


_CLIP_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_.-]*$")
_NUMBER = re.compile(r"^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$")
_TIME = re.compile(r"^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)(ms|s)$")


def _tag(element: ET.Element) -> str:
    return element.tag.rsplit("}", 1)[-1]


def _finite_number(text: str, label: str) -> float:
    value = text.strip()
    if not _NUMBER.fullmatch(value):
        raise ConversionError(f"{label}: expected a finite unitless number")
    result = float(value)
    if not math.isfinite(result):
        raise ConversionError(f"{label}: expected a finite unitless number")
    return result


def _duration(text: str, label: str) -> float:
    match = _TIME.fullmatch(text.strip())
    if match is None:
        raise ConversionError(
            f"{label}: duration must use seconds (s) or milliseconds (ms)"
        )
    value = float(match.group(1))
    if match.group(2) == "ms":
        value /= 1000.0
    if not math.isfinite(value) or value <= 0:
        raise ConversionError(f"{label}: duration must be finite and positive")
    return value


def _begin(text: str, label: str) -> None:
    value = text.strip()
    if value == "indefinite":
        return
    match = _TIME.fullmatch(value)
    if value == "0" or (match is not None and float(match.group(1)) == 0):
        return
    raise ConversionError(
        f"{label}: begin must be 'indefinite' or zero; event and offset triggers are unsupported"
    )


def _scale(text: str, label: str) -> list[float]:
    parts = [part for part in re.split(r"[ ,]+", text.strip()) if part]
    if len(parts) not in (1, 2):
        raise ConversionError(f"{label}: scale must contain one or two numbers")
    values = [_finite_number(part, label) for part in parts]
    if len(values) == 1:
        values *= 2
    if any(value <= 0 for value in values):
        raise ConversionError(f"{label}: scale values must be positive")
    return values


def _opacity(text: str, label: str) -> float:
    value = _finite_number(text, label)
    if not 0 <= value <= 1:
        raise ConversionError(f"{label}: opacity must be within 0..1")
    return value


def _values(element: ET.Element, property_name: str, label: str) -> list[object]:
    has_values = "values" in element.attrib
    has_pair = "from" in element.attrib or "to" in element.attrib
    if has_values == has_pair:
        raise ConversionError(f"{label}: specify either values or both from/to")
    if has_values:
        raw_values = [part.strip() for part in element.attrib["values"].split(";")]
        if len(raw_values) < 2 or any(not part for part in raw_values):
            raise ConversionError(f"{label}.values: expected at least two values")
    else:
        if "from" not in element.attrib or "to" not in element.attrib:
            raise ConversionError(f"{label}: from and to must be specified together")
        raw_values = [element.attrib["from"], element.attrib["to"]]
    parser = _scale if property_name == "scale" else _opacity
    return [parser(value, f"{label}.values") for value in raw_values]


def _key_times(
    element: ET.Element, count: int, calc_mode: str, label: str
) -> list[float]:
    if "keyTimes" not in element.attrib:
        if calc_mode == "discrete":
            return [index / count for index in range(count)]
        return [index / (count - 1) for index in range(count)]
    raw = [part.strip() for part in element.attrib["keyTimes"].split(";")]
    if len(raw) != count:
        raise ConversionError(f"{label}.keyTimes: expected one time per value")
    times = [_finite_number(part, f"{label}.keyTimes") for part in raw]
    if times[0] != 0:
        raise ConversionError(f"{label}.keyTimes: first time must be 0")
    if any(time < 0 or time > 1 for time in times):
        raise ConversionError(f"{label}.keyTimes: times must be within 0..1")
    if calc_mode == "linear" and times[-1] != 1:
        raise ConversionError(f"{label}.keyTimes: linear animation must end at 1")
    if any(left >= right for left, right in pairwise(times)):
        raise ConversionError(f"{label}.keyTimes: times must be strictly increasing")
    return times


def _track(
    element: ET.Element, index: int
) -> tuple[str, str, float, dict[str, object]]:
    tag = _tag(element)
    label = f"svg/{tag}[{index}]"
    allowed = {
        "attributeName",
        "begin",
        "dur",
        "values",
        "from",
        "to",
        "keyTimes",
        "calcMode",
        "fill",
        "data-ipp-clip",
    }
    if tag == "animateTransform":
        allowed.add("type")
    unsupported = sorted(set(element.attrib) - allowed)
    if unsupported:
        raise ConversionError(f"{label}: unsupported attribute {unsupported[0]!r}")
    if list(element):
        raise ConversionError(f"{label}: child elements are unsupported")

    if tag == "animate":
        if element.attrib.get("attributeName") != "opacity":
            raise ConversionError(f"{label}: only attributeName='opacity' is supported")
        property_name = "opacity"
    else:
        if element.attrib.get("attributeName") != "transform":
            raise ConversionError(
                f"{label}: animateTransform requires attributeName='transform'"
            )
        if element.attrib.get("type") != "scale":
            raise ConversionError(
                f"{label}: only animateTransform type='scale' is supported"
            )
        property_name = "scale"

    clip_name = element.attrib.get("data-ipp-clip", "default").strip()
    if not _CLIP_NAME.fullmatch(clip_name):
        raise ConversionError(f"{label}.data-ipp-clip: invalid clip name {clip_name!r}")
    _begin(element.attrib.get("begin", "0"), f"{label}.begin")
    duration = _duration(element.attrib.get("dur", ""), f"{label}.dur")
    if element.attrib.get("fill") != "freeze":
        raise ConversionError(f"{label}.fill: only fill='freeze' is supported")
    calc_mode = element.attrib.get("calcMode", "linear")
    if calc_mode not in ("linear", "discrete"):
        raise ConversionError(
            f"{label}.calcMode: only linear and discrete are supported"
        )
    values = _values(element, property_name, label)
    times = _key_times(element, len(values), calc_mode, label)
    track = {
        "property": property_name,
        "interpolation": "step" if calc_mode == "discrete" else "linear",
        "keys": [
            {"time": duration * time, "value": value}
            for time, value in zip(times, values, strict=True)
        ],
    }
    return clip_name, property_name, duration, track


def convert_svg_animation(
    source: Path,
    drawing_destination: Path,
    animation_destination: Path,
    tolerance: float = 0.001,
) -> None:
    """Write a static IPPD drawing and its validated item-animation manifest."""
    root = ET.parse(source).getroot()
    if _tag(root) != "svg":
        raise ConversionError("root: expected svg")

    animations = {"animate", "animateTransform"}
    direct = [child for child in root if _tag(child) in animations]
    if "transform" in root.attrib and any(
        _tag(child) == "animateTransform" for child in direct
    ):
        raise ConversionError(
            "svg.transform: a root static transform cannot be combined with "
            "animateTransform replacement semantics"
        )
    direct_ids = {id(child) for child in direct}
    for element in root.iter():
        if _tag(element) in animations and id(element) not in direct_ids:
            raise ConversionError(
                f"{_tag(element)}: animation elements must be direct children of the root svg"
            )

    clips: dict[str, dict[str, object]] = {}
    properties: dict[str, set[str]] = {}
    for index, element in enumerate(direct):
        clip_name, property_name, duration, track = _track(element, index)
        clip = clips.setdefault(clip_name, {"duration": duration, "tracks": []})
        if not math.isclose(
            float(clip["duration"]), duration, rel_tol=0, abs_tol=1e-12
        ):
            raise ConversionError(
                f"clip {clip_name!r}: all tracks must have the same duration"
            )
        clip_properties = properties.setdefault(clip_name, set())
        if property_name in clip_properties:
            raise ConversionError(
                f"clip {clip_name!r}: duplicate {property_name!r} track"
            )
        clip_properties.add(property_name)
        tracks = clip["tracks"]
        assert isinstance(tracks, list)
        tracks.append(track)

    static_root = deepcopy(root)
    for child in list(static_root):
        if _tag(child) in animations:
            static_root.remove(child)
    with tempfile.TemporaryDirectory() as directory:
        static_source = Path(directory) / source.name
        ET.ElementTree(static_root).write(
            static_source, encoding="utf-8", xml_declaration=True
        )
        convert_svg(static_source, drawing_destination, tolerance)

    manifest = {"version": 1, "clips": clips}
    animation_destination.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("drawing_destination", type=Path)
    parser.add_argument("animation_destination", type=Path)
    parser.add_argument("--tolerance", type=float, default=0.001)
    args = parser.parse_args()
    convert_svg_animation(
        args.source,
        args.drawing_destination,
        args.animation_destination,
        args.tolerance,
    )


if __name__ == "__main__":
    main()
