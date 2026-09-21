from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.convert_surface_asset import ConversionError, convert_svg
from tools.convert_svg_animation import convert_svg_animation


class SvgAnimationConverterTests(unittest.TestCase):
    def convert(self, source_text: str):
        directory = tempfile.TemporaryDirectory()
        root = Path(directory.name)
        source = root / "source.svg"
        drawing = root / "drawing.ippd"
        animation = root / "animation.json"
        source.write_text(source_text, encoding="utf-8")
        convert_svg_animation(source, drawing, animation)
        return directory, drawing.read_bytes(), json.loads(animation.read_text())

    def test_exports_named_scale_and_opacity_tracks(self) -> None:
        directory, drawing, manifest = self.convert(
            """<svg viewBox="-1 -1 2 2">
              <circle r=".8" fill="#2468a0"/>
              <animateTransform attributeName="transform" type="scale"
                begin="indefinite" dur="160ms" values="1; 1.03 1.04; 1.06"
                keyTimes="0; .25; 1" calcMode="linear" fill="freeze"
                data-ipp-clip="hover"/>
              <animate attributeName="opacity" begin="0s" dur=".16s"
                from=".7" to="1" calcMode="discrete" fill="freeze"
                data-ipp-clip="hover"/>
            </svg>"""
        )
        self.addCleanup(directory.cleanup)
        self.assertEqual(drawing[:4], b"IPPD")
        self.assertEqual(manifest["version"], 1)
        clip = manifest["clips"]["hover"]
        self.assertEqual(clip["duration"], 0.16)
        self.assertEqual(
            clip["tracks"][0],
            {
                "property": "scale",
                "interpolation": "linear",
                "keys": [
                    {"time": 0.0, "value": [1.0, 1.0]},
                    {"time": 0.04, "value": [1.03, 1.04]},
                    {"time": 0.16, "value": [1.06, 1.06]},
                ],
            },
        )
        self.assertEqual(clip["tracks"][1]["property"], "opacity")
        self.assertEqual(clip["tracks"][1]["interpolation"], "step")
        self.assertEqual(
            [key["time"] for key in clip["tracks"][1]["keys"]], [0.0, 0.08]
        )
        self.assertEqual(
            [key["value"] for key in clip["tracks"][1]["keys"]], [0.7, 1.0]
        )

    def test_animation_does_not_change_static_drawing(self) -> None:
        static_text = '<svg viewBox="0 0 2 2"><path fill="red" d="M0 0h2v2z"/></svg>'
        animated_text = static_text.replace(
            "</svg>",
            '<animate attributeName="opacity" dur="1s" values=".7;1" fill="freeze"/></svg>',
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            static_source = root / "static.svg"
            static_drawing = root / "static.ippd"
            animated_source = root / "animated.svg"
            animated_drawing = root / "animated.ippd"
            manifest = root / "animated.json"
            static_source.write_text(static_text)
            animated_source.write_text(animated_text)
            convert_svg(static_source, static_drawing, 0.001)
            convert_svg_animation(animated_source, animated_drawing, manifest)
            self.assertEqual(static_drawing.read_bytes(), animated_drawing.read_bytes())
            self.assertIn("default", json.loads(manifest.read_text())["clips"])

    def test_static_svg_emits_empty_clips(self) -> None:
        directory, _, manifest = self.convert(
            '<svg viewBox="0 0 1 1"><rect width="1" height="1"/></svg>'
        )
        self.addCleanup(directory.cleanup)
        self.assertEqual(manifest, {"version": 1, "clips": {}})

    def test_rejects_unsupported_semantics_with_stable_diagnostics(self) -> None:
        cases = [
            (
                '<g><animate attributeName="opacity" dur="1s" values="0;1" fill="freeze"/></g>',
                "direct children",
            ),
            (
                '<animateTransform attributeName="transform" type="rotate" dur="1s" values="0;1" fill="freeze"/>',
                "type='scale'",
            ),
            (
                '<animate attributeName="opacity" begin="click" dur="1s" values="0;1" fill="freeze"/>',
                "event and offset triggers",
            ),
            (
                '<animate attributeName="opacity" dur="1s" values="0;1" repeatCount="2" fill="freeze"/>',
                "unsupported attribute 'repeatCount'",
            ),
            (
                '<animate attributeName="opacity" dur="1s" values="0;1" fill="remove"/>',
                "fill='freeze'",
            ),
            (
                '<animate attributeName="opacity" dur="1s" values="0;1" keyTimes="0;.3" fill="freeze"/>',
                "linear animation must end at 1",
            ),
            (
                '<animateTransform attributeName="transform" type="scale" dur="1s" values="1;2" fill="freeze"/>',
                "root static transform",
            ),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (animation, message) in enumerate(cases):
                with self.subTest(animation=animation):
                    source = root / f"case-{index}.svg"
                    root_transform = ' transform="translate(1 1)"' if index == 6 else ""
                    source.write_text(
                        f'<svg viewBox="0 0 1 1"{root_transform}><rect width="1" height="1"/>{animation}</svg>'
                    )
                    with self.assertRaisesRegex(ConversionError, message):
                        convert_svg_animation(
                            source,
                            root / f"case-{index}.ippd",
                            root / f"case-{index}.json",
                        )

    def test_discrete_explicit_final_key_time_may_precede_end(self) -> None:
        directory, _, manifest = self.convert(
            '<svg viewBox="0 0 1 1"><rect width="1" height="1"/>'
            '<animate attributeName="opacity" dur="1s" values="0;.5;1" '
            'keyTimes="0;.25;.75" calcMode="discrete" fill="freeze"/></svg>'
        )
        self.addCleanup(directory.cleanup)
        keys = manifest["clips"]["default"]["tracks"][0]["keys"]
        self.assertEqual([key["time"] for key in keys], [0.0, 0.25, 0.75])

    def test_rejects_duplicate_tracks_and_duration_mismatch(self) -> None:
        cases = [
            (
                (
                    '<animate attributeName="opacity" dur="1s" values="0;1" fill="freeze" data-ipp-clip="hover"/>'
                    '<animate attributeName="opacity" dur="1s" values=".2;.8" fill="freeze" data-ipp-clip="hover"/>'
                ),
                "duplicate 'opacity' track",
            ),
            (
                (
                    '<animate attributeName="opacity" dur="1s" values="0;1" fill="freeze" data-ipp-clip="hover"/>'
                    '<animateTransform attributeName="transform" type="scale" dur="2s" values="1;2" fill="freeze" data-ipp-clip="hover"/>'
                ),
                "same duration",
            ),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (animations, message) in enumerate(cases):
                source = root / f"invalid-{index}.svg"
                source.write_text(
                    f'<svg viewBox="0 0 1 1"><rect width="1" height="1"/>{animations}</svg>'
                )
                with self.assertRaisesRegex(ConversionError, message):
                    convert_svg_animation(
                        source, root / "drawing.ippd", root / "animation.json"
                    )


if __name__ == "__main__":
    unittest.main()
