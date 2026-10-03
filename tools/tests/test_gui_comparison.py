"""Comparison identity excludes dynamic counters and rejects incomparable settings."""

from copy import deepcopy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.gui_comparison import compatibility, observed_compositor, timing_values


def report():
    return {
        "workloadSha256": "fixture",
        "fixture": {
            "sha256": "served-fixture",
            "sources": [{"path": "fixture.tsx", "sha256": "fixed-workload"}],
        },
        "assets": [{"path": "font.ippf", "sha256": "font"}],
        "machine": {"cpu": "cpu"},
        "browser": "chromium",
        "build": {
            "arrangement": "native-gles",
            "sourceMatches": True,
            "contractSha256": "contract",
            "clientArtifacts": [{"name": "generated.js", "sha256": "client"}],
        },
        "measurementMode": "post-admission-draw",
        "repetitions": 1,
        "samplesPerSweep": 1,
        "device": {"unmaskedRenderer": "gpu", "shaderProgramsCreated": 10},
        "softwareRenderer": False,
        "compositor": {
            "settings": {"gui": "runtime-default"},
            "observed": [
                {
                    "sweep": "panel",
                    "surfaces": {
                        "surfaceCaches": [
                            {
                                "entity": "1",
                                "mode": "reused",
                                "band": 1,
                                "width": 256,
                                "height": 128,
                            }
                        ]
                    },
                }
            ],
        },
        "sweeps": [
            {
                "sweep": {"name": "panel"},
                "timings": {
                    "edit": {
                        "updateMs": {"medianMs": 3.0},
                        "updateToFrameMs": {"medianMs": 6.0},
                    }
                },
            }
        ],
    }


class GuiComparisonTests(unittest.TestCase):
    def test_matching_settings_ignore_dynamic_resources(self):
        first = report()
        second = deepcopy(first)
        second["device"]["shaderProgramsCreated"] = 99
        second["source"] = {"revision": "different"}
        self.assertEqual(compatibility(first), compatibility(second))

    def test_renderer_compositor_and_workload_must_match(self):
        for key, replacement in (
            ("workloadSha256", "new"),
            ("device", {"unmaskedRenderer": "other gpu"}),
            ("compositor", {"settings": {"gui": "cached"}}),
        ):
            second = report()
            second[key] = replacement
            self.assertNotEqual(compatibility(report()), compatibility(second))

    def test_stale_or_unverified_build_is_rejected(self):
        for match in (False, None):
            second = report()
            second["build"]["sourceMatches"] = match
            with self.assertRaisesRegex(ValueError, "build/current-source"):
                compatibility(second)

    def test_fixture_sources_match_but_product_bytes_may_differ(self):
        first = report()
        second = deepcopy(first)
        second["fixture"]["sources"][0]["sha256"] = "changed"
        self.assertNotEqual(compatibility(first), compatibility(second))
        second = deepcopy(first)
        second["build"]["clientArtifacts"][0]["sha256"] = "changed"
        second["fixture"]["sha256"] = "changed-product-bundle"
        self.assertEqual(compatibility(first), compatibility(second))
        second = deepcopy(first)
        second["compositor"]["settings"]["chromium"] = {
            "args": ["--disable-vulkan-surface"]
        }
        self.assertNotEqual(compatibility(first), compatibility(second))

    def test_cache_frame_actions_normalize_but_resolution_and_mode_do_not(self):
        first = report()
        second = deepcopy(first)
        cache = second["compositor"]["observed"][0]["surfaces"]["surfaceCaches"][0]
        cache.update({"entity": "99", "mode": "repainted"})
        self.assertEqual(observed_compositor(first), observed_compositor(second))
        cache["width"] = 512
        self.assertNotEqual(observed_compositor(first), observed_compositor(second))
        cache.update({"width": 256, "mode": "fallback"})
        self.assertNotEqual(observed_compositor(first), observed_compositor(second))

    def test_timing_values_preserve_metrics(self):
        self.assertEqual(
            timing_values(report()),
            {
                "panel:edit:updateMs:median": [3.0],
                "panel:edit:updateToFrameMs:median": [6.0],
            },
        )


if __name__ == "__main__":
    unittest.main()
