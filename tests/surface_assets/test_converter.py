from __future__ import annotations

import math
import struct
import tempfile
import unittest
from pathlib import Path

from fontTools.ttLib import TTFont

from tools.convert_surface_asset import ConversionError, convert_font, convert_svg
from tools.font_sources import font_source

ROOT = Path(__file__).parents[2]
SVG_SOURCES = ROOT / "examples" / "surface-terminal" / "authoring" / "svg"


def decode_font(data: bytes):
    glyph_count, cmap_count, kerning_count = struct.unpack_from("<3I", data, 24)
    offset = 36
    glyph_contours = []
    for _ in range(glyph_count):
        contour_count = struct.unpack_from("<I", data, offset + 24)[0]
        offset += 28
        glyph_contours.append(contour_count)
        for _ in range(contour_count):
            segment_count = struct.unpack_from("<I", data, offset + 8)[0]
            offset += 12
            for _ in range(segment_count):
                kind = data[offset]
                offset += 12 if kind == 0 else 20
    cmap = {}
    for _ in range(cmap_count):
        codepoint, glyph_id = struct.unpack_from("<II", data, offset)
        offset += 8
        cmap[codepoint] = glyph_id
    kerning = {}
    for _ in range(kerning_count):
        left, right, adjustment = struct.unpack_from("<IIf", data, offset)
        offset += 12
        kerning[(left, right)] = adjustment
    if offset != len(data):
        raise AssertionError("decoder did not consume font")
    return glyph_contours, cmap, kerning


def decode_drawing(data: bytes):
    layer_count = struct.unpack_from("<I", data, 44)[0]
    offset = 48
    layers = []
    for _ in range(layer_count):
        color = tuple(data[offset : offset + 4])
        rule = data[offset + 4]
        contour_count = struct.unpack_from("<I", data, offset + 8)[0]
        offset += 12
        contours = []
        for _ in range(contour_count):
            start = struct.unpack_from("<2f", data, offset)
            segment_count = struct.unpack_from("<I", data, offset + 8)[0]
            offset += 12
            segments = []
            for _ in range(segment_count):
                kind = data[offset]
                count = 2 if kind == 0 else 4
                values = struct.unpack_from(f"<{count}f", data, offset + 4)
                offset += 4 + 4 * count
                segments.append((kind, values))
            contours.append((start, segments))
        layers.append((color, rule, contours))
    if offset != len(data):
        raise AssertionError("decoder did not consume drawing")
    return layers


class SurfaceConverterTests(unittest.TestCase):
    def test_real_static_font_is_deterministic_and_keeps_notdef(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.ippf"
            second = Path(directory) / "second.ippf"
            source_path = font_source("adwaita-mono")
            convert_font(source_path, first)
            convert_font(source_path, second)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            data = first.read_bytes()
            self.assertEqual(data[:8], b"IPPF\x01\x00\x00\x00")
            self.assertGreater(struct.unpack_from("<I", data, 28)[0], 100)
            contours, cmap, _ = decode_font(data)
            source = TTFont(source_path)
            composite_id = source.getGlyphID("quotedbl")
            self.assertTrue(source["glyf"]["quotedbl"].isComposite())
            self.assertGreater(contours[composite_id], 1)
            self.assertEqual(cmap[ord("A")], source.getGlyphID("A"))

    def test_real_proportional_font_extracts_default_kern_feature(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "noto.ippf"
            convert_font(font_source("noto-sans"), output)
            _, cmap, kerning = decode_font(output.read_bytes())
            self.assertEqual(kerning[(cmap[ord("A")], cmap[ord("V")])], -40.0)

    def test_svg_converts_fills_curves_and_expanded_stroke(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "icon.ippd"
            convert_svg(SVG_SOURCES / "icon.svg", output, 0.025)
            data = output.read_bytes()
            self.assertEqual(data[:8], b"IPPD\x01\x00\x00\x00")
            layers = decode_drawing(data)
            self.assertEqual(len(layers), 3)
            self.assertEqual(layers[0][:2], ((36, 104, 160, 255), 1))
            self.assertGreater(len(layers[2][2]), 4)
            self.assertTrue(
                all(
                    segment[0] in (0, 1)
                    for contour in layers[2][2]
                    for segment in contour[1]
                )
            )

    def test_arc_error_is_bounded_in_output_coordinates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "circle.svg"
            source.write_text(
                '<svg viewBox="-11 -11 22 22"><circle cx="0" cy="0" r="10"/></svg>'
            )
            output = Path(directory) / "circle.ippd"
            convert_svg(source, output, 0.01)
            contour = decode_drawing(output.read_bytes())[0][2][0]
            previous = contour[0]
            maximum_error = 0.0
            for kind, values in contour[1]:
                if kind == 0:
                    samples = [(values[0], values[1])]
                else:
                    control = values[:2]
                    end = values[2:]
                    samples = []
                    for index in range(1, 17):
                        t = index / 16
                        samples.append(
                            (
                                (1 - t) * (1 - t) * previous[0]
                                + 2 * (1 - t) * t * control[0]
                                + t * t * end[0],
                                (1 - t) * (1 - t) * previous[1]
                                + 2 * (1 - t) * t * control[1]
                                + t * t * end[1],
                            )
                        )
                for point in samples:
                    maximum_error = max(maximum_error, abs(math.hypot(*point) - 10.0))
                previous = (values[-2], values[-1])
            self.assertLessEqual(maximum_error, 0.0101)

    def test_defs_are_validated_but_not_painted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "defs.svg"
            source.write_text(
                '<svg viewBox="0 0 2 2"><defs><path fill="red" d="M0 0h2v2z"/></defs><path fill="blue" d="M0 0h1v1z"/></svg>'
            )
            output = Path(directory) / "defs.ippd"
            convert_svg(source, output, 0.05)
            self.assertEqual(len(decode_drawing(output.read_bytes())), 1)

    def test_svg_rejects_unsupported_visual_element_with_name(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "bad.svg"
            source.write_text('<svg viewBox="0 0 1 1"><linearGradient/></svg>')
            with self.assertRaisesRegex(ConversionError, "linearGradient"):
                convert_svg(source, Path(directory) / "bad.ippd", 0.05)

    def test_svg_rejects_isolated_opacity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "bad.svg"
            source.write_text('<svg viewBox="0 0 1 1"><g opacity=".5"/></svg>')
            with self.assertRaisesRegex(ConversionError, "opacity"):
                convert_svg(source, Path(directory) / "bad.ippd", 0.05)

    def test_svg_rejects_ignored_visual_semantics_and_malformed_transforms(
        self,
    ) -> None:
        cases = [
            ('<path style="filter:url(#f)"/>', "filter"),
            ('<path display="none"/>', "display"),
            ('<path vector-effect="non-scaling-stroke"/>', "vector-effect"),
            ('<path marker-start="url(#m)"/>', "marker-start"),
            ('<g transform="translate(1) garbage"/>', "transform"),
            ('<svg viewBox="0 0 1 1"/>', "nested"),
        ]
        with tempfile.TemporaryDirectory() as directory:
            for index, (content, message) in enumerate(cases):
                with self.subTest(content=content):
                    source = Path(directory) / f"bad-{index}.svg"
                    source.write_text(f'<svg viewBox="0 0 1 1">{content}</svg>')
                    with self.assertRaisesRegex(ConversionError, message):
                        convert_svg(source, Path(directory) / f"bad-{index}.ippd", 0.05)


if __name__ == "__main__":
    unittest.main()
