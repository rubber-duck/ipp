"""Changed-file selection of the suites and checks that exercise each source."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.catalog import suite_ids
from pipeline.model import ROOT
from pipeline.registries import GLES_CHECKS
from pipeline.selection import affected


class SelectionTests(unittest.TestCase):
    def test_blender_changes_select_all_declared_source_owners(self):
        for source in (
            "integrations/blender/ipp_blender/exporter/particles.py",
            "integrations/blender/client/adapter.ts",
            "tests/blender/particle_oracle.py",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(
                    set(suite_ids(["blender", "particles-blender"])).issubset(ids)
                )
                self.assertIn("test:dist/tests/blender/particles-blender.test.js", ids)

    def test_retained_gui_participant_changes_select_the_suite(self):
        for source in (
            "crates/ipp-render-gl/src/services/render/retained/glyph_atlas.rs",
            "crates/ipp-core/src/world/systems/surface/mod.rs",
            "crates/ipp-core/src/services/asset_management/mod.rs",
            "packages/ipp-client/src/render-worker.ts",
            "examples/surface-terminal/workload.ts",
            "tests/harness/browser.ts",
            "tests/gui/retained/scenarios/retained-gui.ts",
            "tests/gui/retained/support/retained-gui-environment.ts",
            "tests/surfaces/pages/surface.tsx",
            "tests/performance/retained-gui.ts",
            "tools/assets/surface_assets.py",
            "crates/ipp-wasm/src/services/render/service.rs",
            "packages/ipp-client/tools/assemble.mjs",
            "tools/build/verify-browser.mjs",
            "packages/ipp-react/src/gui/theme.ts",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(suite_ids(["retained-gui"])).issubset(ids))

    def test_surface_cache_participant_changes_select_the_suite(self):
        for source in (
            "crates/ipp-render-gl/src/services/render/surface/texture_cache.rs",
            "crates/ipp-render-gl/src/services/render/device/webgl/webgl.ts",
            "crates/ipp-core/src/world/systems/surface/cache_policy.rs",
            "crates/ipp-core/src/world/systems/render/system.rs",
            "packages/ipp-client/src/render-worker.ts",
            "crates/ipp-wasm/src/services/render/service.rs",
            "tools/build/verify-browser.mjs",
            "tests/surfaces/support/error-check-bridge.ts",
            "tests/surfaces/support/surface-cache-bridge.ts",
            "tests/surfaces/scenarios/surface-cache.ts",
            "tests/surfaces/support/surface-cache-environment.ts",
            "tests/surfaces/surface-cache.test.ts",
            "tests/surfaces/pages/surface.tsx",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(suite_ids(["surface-cache"])).issubset(ids))

    def test_surface_cache_gles_check_follows_its_probe_and_runner(self):
        for source in (
            "crates/ipp-render-gl/examples/egl_surface_cache.rs",
            "crates/ipp-render-gl/examples/smoke/surface_cache_target.rs",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertIn("check:gles-surface-cache", ids)
        ids, _ = affected(["crates/ipp-render-gl/examples/egl_surface_cache.rs"], [])
        self.assertEqual(
            [id_ for id_ in ids if id_.startswith("check:gles-")],
            ["check:gles-surface-cache"],
        )

    def test_gallery_gui_participant_changes_select_the_suite(self):
        for source in (
            "examples/world-gallery/worlds/gui/dashboard.tsx",
            "packages/ipp-react/src/gui/theme.ts",
            "packages/ipp-client/tools/assemble.mjs",
            "tools/build/verify-browser.mjs",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(suite_ids(["gallery-gui"])).issubset(ids))

    def test_gui_surface_and_schema_row_sources_select_their_suites(self):
        # Test targets, shared scenarios and schema-row participants each select
        # the suites whose commands or scenarios exercise them.
        owners = {
            "crates/ipp-core/tests/schema_rows.rs": ["gui"],
            "crates/ipp-core/src/commands/command.rs": ["gui"],
            "crates/ipp-core/src/components/mod.rs": ["gui"],
            "crates/ipp-core/src/components/registry.rs": ["gui"],
            "crates/ipp-core/src/components/lifecycle.rs": ["gui"],
            "crates/ipp-core/src/components/rows/table.rs": ["gui"],
            "crates/ipp-core/src/components/rows/fixture.rs": ["gui"],
            "crates/ipp-core/src/components/rows/table_tests.rs": ["gui"],
            "crates/ipp-core/src/components/schema.rs": ["gui"],
            "crates/ipp-core/src/components/dynamic_properties/storage.rs": ["gui"],
            "crates/ipp-core/src/world/component_state/staging_tests.rs": ["gui"],
            "tools/ipp-schema-derive/src/lib.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/schema_component.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/component_registry.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/schema_row.rs": ["gui", "contracts"],
            "tools/ipp-schema-gen/tests/target-contract.mjs": ["contracts"],
            "crates/ipp-protocol/tests/generated-client.mjs": ["client", "gui"],
            "tests/surfaces/scenarios/surface-lifecycle.ts": [
                "gui",
                "surfaces",
                "surface-cache",
                "retained-gui",
            ],
            "crates/ipp-core/src/world/systems/skeleton/mod.rs": ["skinning"],
            "crates/ipp-core/src/world/systems/skeleton/component.rs": ["skinning"],
            "crates/ipp-core/src/world/systems/skeleton/update.rs": ["skinning"],
            "crates/ipp-core/tests/skeleton.rs": ["skinning"],
            "crates/ipp-core/tests/skeleton_animation.rs": ["skinning"],
            "tests/rendering/pages/skinning.ts": ["skinning", "geometry"],
            "tests/rendering/pages/mesh-poses.ts": ["mesh-poses"],
            "tests/rendering/pages/custom-materials.ts": ["custom-materials"],
            "tests/blender/pages/blender-viewer.tsx": ["blender", "particles-blender"],
            "crates/ipp-core/src/services/world_serialization/validation.rs": [
                "snapshots"
            ],
            "crates/ipp-core/src/services/world_serialization/container.rs": [
                "snapshots"
            ],
            "crates/ipp-render-gl/Cargo.toml": ["surface-cache", "render-residency"],
        }
        for source, suites in owners.items():
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(suite_ids(suites)).issubset(ids))

        # The renderer manifest backs every native GLES check; the shared
        # smoke module backs every renderer example that includes it.
        checks = {record["id"] for record in GLES_CHECKS}
        smoke_examples = {
            record["id"]
            for record in GLES_CHECKS
            if record["command"][:3] == ["cargo", "run", "-p"]
            and record["command"][3] == "ipp-render-gl"
        } - {"check:gles-asset-readback"}
        # The readback probe includes only the EGL driver directly; changing the
        # broader smoke module must not select this independent device check.
        for source, expected in (
            ("crates/ipp-render-gl/Cargo.toml", checks),
            ("crates/ipp-render-gl/examples/smoke/mod.rs", smoke_examples),
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(expected.issubset(ids), expected - set(ids))

    def test_asset_readback_check_follows_its_probe_driver_and_device(self):
        readback = "check:gles-asset-readback"
        for source in (
            "crates/ipp-render-gl/examples/egl_texture_readback.rs",
            "crates/ipp-render-gl/examples/smoke/egl.rs",
            "crates/ipp-render-gl/src/services/render/device/gles/texture_readback.rs",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertIn(readback, ids)
        ids, _ = affected(["crates/ipp-render-gl/examples/egl_texture_readback.rs"], [])
        self.assertEqual(
            [id_ for id_ in ids if id_.startswith("check:gles-")],
            [readback],
        )
        ids, _ = affected(["crates/ipp-render-gl/examples/smoke/mod.rs"], [])
        self.assertNotIn(readback, ids)

    def test_wasm_services_select_the_browser_render_suites(self):
        host, _ = affected(["crates/ipp-wasm/src/services/host_services.rs"], [])
        self.assertTrue(set(suite_ids(["browser", "render"])).issubset(host))
        self.assertFalse(set(suite_ids(["retained-gui"])).issubset(host))
        render, _ = affected(["crates/ipp-wasm/src/services/render/service.rs"], [])
        self.assertTrue(
            set(suite_ids(["browser", "render", "retained-gui"])).issubset(render)
        )

    def test_owners_add_to_area_rules(self):
        ids, _ = affected(["packages/ipp-client/src/render-worker.ts"], [])
        self.assertTrue(
            set(suite_ids(["client", "native", "browser", "retained-gui"])).issubset(
                ids
            )
        )

    def test_workspace_check_script_selects_its_check(self):
        ids, _ = affected(["tools/check_workspace.py"], [])
        self.assertIn("check:workspace", ids)

    def test_repository_check_libraries_select_the_check_and_runner_suite(self):
        ids, _ = affected(["tools/checks/mirrored_limits.py"], [])
        self.assertIn("check:repository", ids)
        self.assertTrue(set(suite_ids(["runner"])).issubset(ids))

    def test_asset_converters_select_the_surfaces_suite(self):
        ids, _ = affected(["tools/assets/convert_surface_asset.py"], [])
        self.assertTrue(set(suite_ids(["surfaces"])).issubset(ids))

    def test_files_without_area_rules_select_their_declared_coverage(self):
        gallery = suite_ids(
            [
                "animation",
                "cameras",
                "render",
                "lighting",
                "gallery-particles",
                "gallery-gui",
                "gallery-gui-camera",
                "gallery-platformer",
            ]
        )
        for source, expected in (
            ("crates/ipp-core/src/lib.rs", suite_ids(["contracts"])),
            (
                "crates/ipp-protocol/tests/manifest-client.mjs",
                ["test:crates/ipp-protocol/tests/manifest-client.mjs"],
            ),
            (
                "crates/ipp-protocol/tests/mesh-client.mjs",
                ["test:crates/ipp-protocol/tests/mesh-client.mjs"],
            ),
            (
                "crates/ipp-render-gl/examples/egl_gui_clips.rs",
                ["check:gles-gui-clips"],
            ),
            (
                "crates/ipp-render-gl/examples/egl_gui_layout.rs",
                ["check:gles-gui-layout"],
            ),
            (
                "crates/ipp-render-gl/tests/mesh_residency.rs",
                suite_ids(["render-residency"]),
            ),
            ("tests/gallery/pages/viewer.ts", gallery),
            ("tests/gallery/drivers/browser-gallery.ts", gallery),
            ("tests/gallery/support/gallery-server.ts", gallery),
            (
                "crates/ipp-core/src/world/mutation/operation.rs",
                suite_ids(["command-streaming", "animation", "gui"]),
            ),
            ("tests/rendering/pages/texture.tsx", suite_ids(["textures"])),
            (
                "tools/assets/gallery_gui_assets.py",
                suite_ids(["gallery-site", "gallery-gui", "gallery-gui-camera"]),
            ),
            (
                "crates/ipp-core/src/world/systems/animation/update.rs",
                suite_ids(["animation"]),
            ),
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(expected).issubset(ids), sorted(ids))

    def test_gles_examples_select_only_the_checks_that_run_them(self):
        ids, _ = affected(["crates/ipp-render-gl/examples/egl_gui_clips.rs"], [])
        self.assertEqual(
            [id_ for id_ in ids if id_.startswith("check:gles-")],
            ["check:gles-gui-clips"],
        )
        ids, _ = affected(["crates/ipp-render-gl/examples/egl_smoke.rs"], [])
        self.assertEqual(
            {id_ for id_ in ids if id_.startswith("check:gles-")},
            {"check:gles-spatial", "check:gles-textures"},
        )

    def test_publication_suite_follows_renderer_library_tests_and_build_inputs(self):
        expected = set(suite_ids(["render-publications"]))
        for source in (
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "crates/ipp-render-gl/src/lib.rs",
            "crates/ipp-render-gl/src/services/render/canvas/scene.rs",
            "crates/ipp-render-gl/src/services/render/shaders/unlit.vert",
            "crates/ipp-render-gl/tests/publication_rendering.rs",
            "crates/ipp-render-gl/tests/mesh_residency.rs",
            "crates/ipp-render-gl/tests/support/canvas.rs",
            "crates/ipp-render-gl/Cargo.toml",
            "crates/ipp-render-gl/build.rs",
            "crates/ipp-core/src/world/systems/render/system.rs",
            "crates/ipp-core/src/host/queries/scene.rs",
            "crates/ipp-core/tests/debug_geometry.rs",
            "crates/ipp-core/tests/attachment_placement.rs",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(expected.issubset(ids), expected - set(ids))

    def test_renderer_probes_do_not_select_the_publication_library_suite(self):
        library_checks = set(suite_ids(["render-publications"]))
        for source in (
            "crates/ipp-render-gl/examples/egl_gui_clips.rs",
            "crates/ipp-render-gl/examples/egl_gui_layout.rs",
            "crates/ipp-render-gl/examples/egl_surface_cache.rs",
            "crates/ipp-render-gl/examples/egl_smoke.rs",
            "crates/ipp-render-gl/examples/egl_publications.rs",
            "crates/ipp-render-gl/examples/smoke/publications.rs",
            "crates/ipp-render-gl/examples/smoke/canvas_publications.rs",
            "crates/ipp-render-gl/examples/smoke/canvas_assets.rs",
            "crates/ipp-render-gl/examples/smoke/gui_publications.rs",
            "crates/ipp-render-gl/examples/smoke/gui_control_publications.rs",
            "crates/ipp-render-gl/examples/smoke/gui_cache_interaction.rs",
            "crates/ipp-render-gl/examples/smoke/surface_visibility.rs",
            "crates/ipp-render-gl/examples/smoke/egl.rs",
            "crates/ipp-render-gl/examples/smoke/mod.rs",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertFalse(library_checks.intersection(ids))

    def test_publication_gles_checks_follow_shared_build_inputs(self):
        expected = {"check:gles-publications"}
        for source in (
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "crates/ipp-render-gl/Cargo.toml",
            "crates/ipp-render-gl/build.rs",
            "crates/ipp-render-gl/src/lib.rs",
            "crates/ipp-render-gl/src/services/render/canvas/scene.rs",
            "crates/ipp-render-gl/src/services/render/shaders/unlit.vert",
            "crates/ipp-render-gl/examples/egl_publications.rs",
            "crates/ipp-render-gl/examples/smoke/publications.rs",
            "crates/ipp-render-gl/examples/smoke/egl.rs",
            "crates/ipp-render-gl/examples/smoke/mod.rs",
        ):
            with self.subTest(source=source):
                self.assertTrue((ROOT / source).is_file())
                ids, _ = affected([source], [])
                self.assertEqual(
                    {
                        task
                        for task in ids
                        if task.startswith("check:gles-publications")
                    },
                    expected,
                )

    def test_publication_gles_check_follows_its_helpers(self):
        for helper in (
            "canvas_assets",
            "canvas_publications",
            "surface_visibility",
            "gui_publications",
            "gui_control_publications",
            "gui_cache_interaction",
        ):
            expected = {"check:gles-publications"}
            with self.subTest(helper=helper):
                source = f"crates/ipp-render-gl/examples/smoke/{helper}.rs"
                self.assertTrue((ROOT / source).is_file())
                ids, _ = affected([source], [])
                self.assertEqual(
                    {task for task in ids if task.startswith("check:gles-")}, expected
                )

    def test_unrelated_renderer_probes_keep_exact_gles_consumers(self):
        for source, expected in (
            ("examples/egl_gui_layout.rs", {"check:gles-gui-layout"}),
            ("examples/smoke/surface_cache_target.rs", {"check:gles-surface-cache"}),
            ("tests/publication_rendering.rs", set()),
            ("tests/support/canvas.rs", set()),
        ):
            with self.subTest(source=source):
                ids, _ = affected([f"crates/ipp-render-gl/{source}"], [])
                self.assertEqual(
                    {task for task in ids if task.startswith("check:gles-")}, expected
                )

    def test_file_source_roots_match_one_path(self):
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["tests/surfaces/pages/surface.tsx.orig"], [])

    def test_source_owner_requires_a_directory_boundary(self):
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["integrations/blender-extra/adapter.ts"], [])

    def test_changed_selection_is_explicit_about_unknown_coverage(self):
        ids, notes = affected(["README.md"], [])
        self.assertIn("check:format-md", ids)
        self.assertFalse(any(id_.startswith("test:") for id_ in ids))
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["crates/ipp-core/src/world/storage.rs"], [])
        ids, notes = affected(["crates/ipp-core/src/world/storage.rs"], ["worlds"])
        self.assertTrue(any("Explicit suites" in note for note in notes))


if __name__ == "__main__":
    unittest.main()
