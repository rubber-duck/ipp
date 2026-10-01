"""Public planning contracts and real subprocess/evidence lifecycle coverage."""

from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.artifacts import source_identity, workspace_lock
from pipeline.catalog import (
    GLES_CHECKS,
    PROFILES,
    REGRESSION_GROUPS,
    SUITES,
    TEST_INPUTS,
    catalog,
    regression_ids,
    regression_group_ids,
    suite_ids,
)
from pipeline.cli import list_selections, make_plan, parser
from pipeline.environment import Requirement, inspect as inspect_environment, probe
from pipeline.model import ROOT, Plan, Task, select
from pipeline.operations import validate_catalog
from pipeline.runner import retry_ids, run_plan
from pipeline.selection import affected


def plan(*args):
    return make_plan(parser().parse_args(args))


class PlanningTests(unittest.TestCase):
    def test_gallery_generates_assets_before_bundling(self):
        for command, target in (
            ("build", "gallery"),
            ("dev", "gallery"),
            ("test", "gallery-site"),
            ("test", "gallery-platformer"),
            ("test", "gallery-gui"),
        ):
            with self.subTest(command=command, target=target):
                selected = plan(command, target)
                ids = [task.id for task in selected.tasks]
                for asset in ("gallery-platformer-assets", "gallery-gui-assets"):
                    self.assertEqual(ids.count(f"build:{asset}"), 1)
                    self.assertLess(
                        ids.index(f"build:{asset}"), ids.index("build:gallery")
                    )
                self.assertLess(
                    ids.index("build:browser:render"),
                    ids.index("build:gallery-platformer-assets"),
                )
                requirements = {
                    name for task in selected.tasks for name in task.requirements
                }
                self.assertTrue({"blender", "browser"}.issubset(requirements))

    def test_gallery_and_surface_ci_prepare_shared_fonts(self):
        for command, target in (
            ("build", "gallery"),
            ("test", "gallery-site"),
            ("test", "surfaces"),
            ("test", "gui"),
        ):
            with self.subTest(command=command, target=target):
                selected = plan(command, target)
                ids = [task.id for task in selected.tasks]
                self.assertEqual(ids.count("build:font-assets"), 1)
                for consumer in (
                    "build:surface-assets",
                    "build:gallery-gui-assets",
                ):
                    if consumer in ids:
                        self.assertLess(
                            ids.index("build:font-assets"), ids.index(consumer)
                        )
                if target in ("gallery", "gallery-site"):
                    self.assertNotIn("build:surface-assets", ids)

    def test_raw_surface_targets_have_independent_strict_builds(self):
        selected = plan(
            "regression",
            "--suite",
            "surfaces",
            "--only",
            "test:surface-cache:browser-surfaces",
        )
        ids = {task.id for task in selected.tasks}
        self.assertIn("build:surface-fixtures", ids)
        self.assertIn("test:surfaces:native", ids)
        self.assertIn("test:surfaces:browser", ids)
        self.assertNotIn("build:typescript", ids)
        self.assertNotIn("build:surface-gui-fixtures", ids)
        gui = plan("regression", "--only", "test:surface-cache:browser-gui")
        self.assertIn("build:surface-gui-fixtures", {task.id for task in gui.tasks})
        self.assertIn("test:surface-cache:browser-gui", gui.requested)

    def test_performance_scene_stays_outside_regression(self):
        selected = plan("benchmark", "native", "--preset", "full", "--culling-views")
        ids = [task.id for task in selected.tasks]
        self.assertIn("build:performance-native", ids)
        self.assertIn("benchmark:scene", ids)
        retained = plan("benchmark", "browser", "--scene", "retained-gui")
        retained_ids = [task.id for task in retained.tasks]
        self.assertIn("benchmark:retained-gui", retained_ids)
        self.assertIn("build:browser:render-instrumentation", retained_ids)
        self.assertNotIn("build:blender-fixtures", retained_ids)
        with self.assertRaises(ValueError):
            plan("benchmark", "native", "--scene", "retained-gui")
        for full in (False, True):
            tasks = catalog("/example/egl")
            regression = select(tasks, regression_ids(tasks, full=full))
            self.assertFalse(
                any(
                    "performance-" in task.id or task.id.startswith("benchmark:")
                    for task in regression
                )
            )
            self.assertFalse(
                any(
                    "stress_scene.py" in part
                    for task in regression
                    for part in task.command
                )
            )

    def test_performance_reuse_and_comparison_options(self):
        selected = plan(
            "benchmark",
            "native",
            "--instrumented",
            "--reuse-build",
            "--reuse-scene",
            "--reuse-import",
        )
        self.assertEqual([task.id for task in selected.tasks], ["benchmark:scene"])
        with self.assertRaises(ValueError):
            plan("benchmark", "native", "--culling-views", "--draw-sweep")
        for backend, flags in (
            ("native", ["--compare-culling"]),
            ("native", ["--group", "32"]),
            ("browser", ["--geometry-index", "flat"]),
            ("browser", ["--skip-moving"]),
            ("browser", ["--allow-software"]),
        ):
            with (
                self.subTest(backend=backend, flags=flags),
                self.assertRaises(ValueError),
            ):
                plan("benchmark", backend, *flags)

    def test_retained_gui_benchmark_rejects_stress_options(self):
        for flags in (
            ["--preset", "smoke"],
            ["--group", "32"],
            ["--instrumented"],
            ["--allow-software"],
            ["--scene-dir", "target/example"],
            ["--geometry-index", "flat"],
            ["--compare-culling"],
            ["--reuse-import"],
        ):
            with (
                self.subTest(flags=flags),
                self.assertRaisesRegex(ValueError, "stress-scene options"),
            ):
                plan("benchmark", "browser", "--scene", "retained-gui", *flags)
        selected = plan(
            "benchmark",
            "browser",
            "--scene",
            "retained-gui",
            "--frames",
            "5",
            "--reuse-build",
        )
        self.assertEqual(
            [task.id for task in selected.tasks], ["benchmark:retained-gui"]
        )
        self.assertEqual(selected.tasks[0].command[2], "5")
        stress = plan("benchmark", "native", "--reuse-build")
        config = json.loads(stress.tasks[-1].command[-1])
        self.assertEqual((config["preset"], config["group"]), ("smoke", 64))

    def test_retained_gui_benchmark_surface_cache_mode(self):
        plain = plan("benchmark", "browser", "--scene", "retained-gui").tasks[-1]
        self.assertNotIn("--surface-cache", plain.command)
        cached = plan(
            "benchmark", "browser", "--scene", "retained-gui", "--surface-cache"
        ).tasks[-1]
        self.assertEqual(cached.id, "benchmark:retained-gui")
        self.assertEqual(cached.command[-1], "--surface-cache")
        self.assertEqual(
            cached.command[3], "target/performance/retained-gui-surface-cache"
        )
        for backend in ("browser", "native"):
            with (
                self.subTest(backend=backend),
                self.assertRaisesRegex(ValueError, "--scene retained-gui"),
            ):
                plan("benchmark", backend, "--surface-cache")

    def test_gui_stress_benchmark_keeps_one_gui_build_per_backend(self):
        browser = plan(
            "benchmark", "browser", "--scene", "gui-stress", "--repetitions", "2"
        ).tasks[-1]
        self.assertEqual(browser.id, "benchmark:gui-stress")
        self.assertEqual(browser.command[2:4], ("browser", "2"))
        # Timing measures the shipped build; the core profile needs instrumentation.
        self.assertIn("build:browser:render", browser.dependencies)
        self.assertIn("build:browser:render-instrumentation", browser.dependencies)
        native = plan(
            "benchmark", "native", "--scene", "gui-stress", "--egl-dir", "/lib64"
        ).tasks[-1]
        self.assertEqual(native.command[2], "native-gles")
        self.assertIn("build:gles-host", native.dependencies)
        for flags in (("--frames", "4"), ("--group", "32"), ("--surface-cache",)):
            with self.subTest(flags=flags), self.assertRaises(ValueError):
                plan("benchmark", "browser", "--scene", "gui-stress", *flags)

    def test_gui_stress_correctness_uses_its_strict_product_and_real_backends(self):
        for backend in ("browser", "native"):
            with self.subTest(backend=backend):
                selected = plan(
                    "regression",
                    "--only",
                    f"test:gui-stress:{backend}",
                    "--egl-dir",
                    "/lib64",
                )
                identifiers = {task.id for task in selected.tasks}
                self.assertIn("build:gui-stress-fixtures", identifiers)
                self.assertIn("build:surface-assets", identifiers)
                self.assertNotIn("build:typescript", identifiers)
                self.assertNotIn("build:surface-fixtures", identifiers)
                self.assertIn(
                    "build:browser:render"
                    if backend == "browser"
                    else "build:gles-host",
                    identifiers,
                )
                self.assertNotIn("build:browser:render-instrumentation", identifiers)
                self.assertNotIn("build:gles-host-instrumentation", identifiers)

    def test_ci_runs_retained_gui_as_its_own_bounded_cached_job(self):
        # Job blocks at two-space indentation below `jobs:`; no YAML dependency.
        jobs: dict[str, list[str]] = {}
        current = None
        lines = (ROOT / ".github/workflows/gallery-pages.yml").read_text()
        for line in lines.split("jobs:\n", 1)[1].splitlines():
            if line.startswith("  ") and not line.startswith("   "):
                current = line.strip().removesuffix(":")
                jobs[current] = []
            elif current:
                jobs[current].append(line.strip())
        retained = jobs["retained-gui"]
        self.assertTrue(any(line.startswith("timeout-minutes: ") for line in retained))
        for step in (
            "run: python tools/ipp.py doctor --for retained-gui surface-cache",
            "run: python tools/ipp.py test retained-gui",
            "run: python tools/ipp.py test surface-cache",
            "~/.cargo/registry/cache/",
            "target/integration-artifacts/retained-gui/",
            "target/integration-artifacts/surface-cache/",
            "if: always()",
        ):
            self.assertIn(step, retained)
        self.assertTrue(
            any(line.startswith("uses: actions/cache@") for line in retained)
        )
        # Deployment waits for the visible retained GUI job, not a hidden step.
        self.assertIn("needs: [build, retained-gui]", jobs["deploy"])
        self.assertFalse(any("retained-gui" in line for line in jobs["build"]))

    def test_every_suite_and_full_selection_resolves(self):
        validate_catalog()
        tasks = catalog("/example/egl")
        full = select(tasks, regression_ids(tasks, full=True))
        self.assertEqual(len(full), len({task.id for task in full}))
        ids = [task.id for task in full]
        for name in SUITES:
            self.assertTrue(set(suite_ids([name])).issubset(ids))
        self.assertIn("check:gles-particles", ids)
        self.assertIn("check:browser-identities", ids)
        self.assertIn("check:contracts", ids)
        self.assertIn("check:contract-identities", ids)

    def test_camera_profiles_and_shared_tests_are_precise(self):
        selected = plan("test", "cameras")
        distributions = {
            task.id.removeprefix("build:browser:")
            for task in selected.tasks
            if task.id.startswith("build:browser:")
        }
        # Only the GPU recovery scenario needs the instrumentation build.
        self.assertEqual(
            distributions, {"headless", "render", "render-instrumentation"}
        )
        worlds = plan("test", "worlds")
        self.assertFalse(
            any(task.id.startswith("build:browser:") for task in worlds.tasks)
        )
        combined = plan("test", "cameras", "worlds", "render")
        self.assertEqual(len(combined.tasks), len({task.id for task in combined.tasks}))
        for index, task in enumerate(combined.tasks):
            self.assertTrue(
                set(task.dependencies).issubset(t.id for t in combined.tasks[:index])
            )

    def test_application_build_does_not_prepare_test_fixtures(self):
        gallery = plan("build", "gallery")
        ids = [task.id for task in gallery.tasks]
        self.assertIn("build:browser:render", ids)
        self.assertNotIn("build:gallery-fixtures", ids)
        self.assertNotIn("build:typescript", ids)
        self.assertEqual(sum(id_.startswith("build:browser:") for id_ in ids), 1)

    def test_typecheck_prepares_its_actual_target(self):
        ids = [task.id for task in plan("check", "typecheck").tasks]
        self.assertEqual(
            ids,
            [
                "build:native",
                "build:client",
                "build:react",
                "build:react-gui-authoring",
                "build:gui-stress-fixtures",
                "check:typecheck",
            ],
        )

    def test_headless_example_build_and_native_cli_share_the_target(self):
        prepared = plan("dev", "headless-client", "--build")
        self.assertEqual(
            [task.id for task in prepared.tasks],
            ["build:native", "build:headless-client"],
        )
        running = plan("dev", "headless-client", "ws://127.0.0.1:9231")
        self.assertEqual(
            running.tasks[-1].command[1:],
            ("target/headless-client/main.js", "ws://127.0.0.1:9231"),
        )
        checked = plan("regression", "--suite", "headless-client")
        self.assertEqual(
            [task.id for task in checked.tasks],
            ["build:native", "build:headless-client", "test:headless-client:native"],
        )
        self.assertFalse(any("browser" in task.requirements for task in checked.tasks))

    def test_mesh_pose_fixtures_need_only_their_node_generator(self):
        selected = plan("build", "mesh-pose-fixtures")
        self.assertEqual(
            [task.id for task in selected.tasks], ["build:mesh-pose-fixtures"]
        )
        self.assertEqual(set(selected.tasks[0].requirements), {"node", "npm"})

    def test_blender_changes_select_all_declared_source_owners(self):
        for source in (
            "integrations/blender/ipp_blender/particles.py",
            "integrations/blender/client/adapter.ts",
            "tests/blender/particle_oracle.py",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(
                    set(suite_ids(["blender", "particles-blender"])).issubset(ids)
                )
                self.assertIn("test:dist/tests/render/particles-blender.test.js", ids)

    def test_retained_gui_participant_changes_select_the_suite(self):
        for source in (
            "crates/ipp-render-gl/src/services/render/glyph_atlas.rs",
            "crates/ipp-core/src/world/systems/surface/mod.rs",
            "crates/ipp-core/src/services/asset_management/mod.rs",
            "packages/ipp-client/src/render-worker.ts",
            "examples/surface-terminal/workload.ts",
            "tests/browser/environment.ts",
            "tests/render/retained-gui-scenario.ts",
            "tests/render/retained-gui-environment.ts",
            "tests/render/surface-fixture.tsx",
            "tests/performance/retained-gui.ts",
            "tools/build_surface_assets.py",
            "crates/ipp-wasm/src/services/render.rs",
            "packages/ipp-client/tools/assemble.mjs",
            "tools/build/verify-browser.mjs",
            "packages/ipp-react/src/gui/theme.ts",
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(set(suite_ids(["retained-gui"])).issubset(ids))

    def test_surface_cache_participant_changes_select_the_suite(self):
        for source in (
            "crates/ipp-render-gl/src/services/render/surface_cache.rs",
            "crates/ipp-render-gl/src/services/render/webgl.ts",
            "crates/ipp-core/src/world/systems/surface/cache_policy.rs",
            "crates/ipp-core/src/world/systems/render/system.rs",
            "packages/ipp-client/src/render-worker.ts",
            "crates/ipp-wasm/src/services/render.rs",
            "tools/build/verify-browser.mjs",
            "tests/render/error-check-bridge.ts",
            "tests/render/surface-cache-bridge.ts",
            "tests/render/surface-cache-scenario.ts",
            "tests/render/surface-cache-environment.ts",
            "tests/render/surface-cache.test.ts",
            "tests/render/surface-fixture.tsx",
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
            "crates/ipp-core/src/commands.rs": ["gui"],
            "crates/ipp-core/src/components/mod.rs": ["gui"],
            "crates/ipp-core/src/components/registry.rs": ["gui"],
            "crates/ipp-core/src/components/lifecycle.rs": ["gui"],
            "crates/ipp-core/src/components/rows.rs": ["gui"],
            "crates/ipp-core/src/components/rows_fixture.rs": ["gui"],
            "crates/ipp-core/src/components/rows_tests.rs": ["gui"],
            "crates/ipp-core/src/components/schema.rs": ["gui"],
            "crates/ipp-core/src/components/dynamic_properties/storage.rs": ["gui"],
            "crates/ipp-core/src/world/component_state/staging_tests.rs": ["gui"],
            "tools/ipp-schema-derive/src/lib.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/component_derive.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/registry_codegen.rs": ["gui", "contracts"],
            "tools/ipp-schema-derive/src/row_derive.rs": ["gui", "contracts"],
            "tools/ipp-schema-gen/tests/target-contract.mjs": ["contracts"],
            "crates/ipp-protocol/tests/generated-client.mjs": ["client", "gui"],
            "tests/integration/surface-scenario.ts": [
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
            "tests/render/skinning-fixture.ts": ["skinning", "geometry"],
            "tests/render/mesh-poses-fixture.ts": ["mesh-poses"],
            "tests/render/custom-materials-fixture.ts": ["custom-materials"],
            "tests/render/blender-fixture.tsx": ["blender", "particles-blender"],
            "crates/ipp-core/src/services/world_serialization/assets.rs": ["snapshots"],
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
        }
        for source, expected in (
            ("crates/ipp-render-gl/Cargo.toml", checks),
            ("crates/ipp-render-gl/examples/smoke/mod.rs", smoke_examples),
        ):
            with self.subTest(source=source):
                ids, _ = affected([source], [])
                self.assertTrue(expected.issubset(ids), expected - set(ids))

    def test_mapped_rust_test_targets_run_in_their_suites(self):
        # Selecting a suite for a Cargo test target is only useful when one of
        # its commands runs that target.
        targets = {
            "gui": ["schema_rows"],
            "skinning": ["skeleton", "skeleton_animation"],
            "snapshots": ["world_persistence"],
        }
        for suite, names in targets.items():
            commands = [entry["command"] for entry in SUITES[suite]["commands"]]
            for name in names:
                with self.subTest(suite=suite, target=name):
                    self.assertTrue(
                        any(
                            command[:3] == ["cargo", "test", "-p"]
                            and "ipp-core" in command
                            and any(
                                command[i : i + 2] == ["--test", name]
                                for i in range(len(command) - 1)
                            )
                            for command in commands
                        )
                    )

    def test_wasm_services_select_the_browser_render_suites(self):
        host, _ = affected(["crates/ipp-wasm/src/services/host.rs"], [])
        self.assertTrue(set(suite_ids(["browser", "render"])).issubset(host))
        self.assertFalse(set(suite_ids(["retained-gui"])).issubset(host))
        render, _ = affected(["crates/ipp-wasm/src/services/render.rs"], [])
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
            ("tests/render/viewer-browser-helper.ts", gallery),
            ("tests/render/gallery-driver.ts", gallery),
            ("tests/render/gallery-server.ts", gallery),
            (
                "crates/ipp-core/src/world/mutation.rs",
                suite_ids(["command-streaming", "animation", "gui"]),
            ),
            ("tests/render/texture-fixture.tsx", suite_ids(["textures"])),
            (
                "tools/build_gallery_gui_assets.py",
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
            "crates/ipp-render-gl/src/services/render/canvas_scene.rs",
            "crates/ipp-render-gl/src/services/render/unlit.vert",
            "crates/ipp-render-gl/tests/publication_rendering.rs",
            "crates/ipp-render-gl/tests/mesh_residency.rs",
            "crates/ipp-render-gl/tests/support/canvas.rs",
            "crates/ipp-render-gl/Cargo.toml",
            "crates/ipp-render-gl/build.rs",
            "crates/ipp-core/src/world/systems/render/system.rs",
            "crates/ipp-core/src/host/scene.rs",
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
            "crates/ipp-render-gl/src/services/render/canvas_scene.rs",
            "crates/ipp-render-gl/src/services/render/unlit.vert",
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

    def test_publication_gles_roots_and_plans_preserve_target_requirements(self):
        for record in GLES_CHECKS:
            if not record["id"].startswith("check:gles-publications"):
                continue
            with self.subTest(check=record["id"]):
                for source in record["sourceRoots"]:
                    path = ROOT / source
                    self.assertTrue(path.exists(), source)
                    self.assertEqual(path.is_dir(), source.endswith("/"), source)
                selected = plan(
                    "regression", "--only", record["id"], "--egl-dir", "/validation/egl"
                )
                consumer = selected.tasks[-1]
                self.assertEqual(consumer.id, record["id"])
                self.assertEqual(
                    consumer.command,
                    tuple(
                        "/validation/egl" if argument == "@egl" else argument
                        for argument in record["command"]
                    ),
                )
                self.assertEqual(consumer.dependencies, tuple(record["dependencies"]))
                self.assertTrue({"rust", "gles"}.issubset(consumer.requirements))
                self.assertIn(
                    "build:surface-assets", [task.id for task in selected.tasks]
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

    def test_native_retained_gui_check_drives_its_client_on_the_selected_egl(self):
        task = catalog("/validation/egl")["check:gles-retained-gui"]
        self.assertEqual(
            task.command[1:],
            ("target/surface-gui-build/retained-gui-native.js", "/validation/egl"),
        )
        self.assertTrue({"gles", "node", "browser"}.issubset(task.requirements))
        # Its hosts are prebuilt, so it never holds the scheduler's Cargo slot.
        self.assertNotIn("rust", task.requirements)
        self.assertIn("build:gles-host-instrumentation", task.dependencies)
        ids, _ = affected(
            ["crates/ipp-server/examples/gles_presentation/channel.rs"], []
        )
        self.assertEqual(
            [id_ for id_ in ids if id_.startswith("check:gles-")],
            ["check:gles-retained-gui"],
        )

    def test_catalog_rejects_invalid_gles_source_roots(self):
        for root in ("../outside.rs", "crates/ipp-render-gl/examples/missing.rs"):
            with (
                self.subTest(root=root),
                patch.dict(GLES_CHECKS[0], sourceRoots=[root]),
            ):
                with self.assertRaisesRegex(ValueError, "invalid source root"):
                    validate_catalog()

    def test_file_source_roots_match_one_path(self):
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["tests/render/surface-fixture.tsx.orig"], [])

    def test_source_owner_requires_a_directory_boundary(self):
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["integrations/blender-extra/adapter.ts"], [])

    def test_catalog_rejects_missing_or_escaping_source_roots(self):
        for root in (
            "../outside/",
            "tests",
            "missing-directory/",
            "tests/render/missing-fixture.tsx",
            "tools/ipp.py/",
            "../tools/ipp.py",
        ):
            with (
                self.subTest(root=root),
                patch.dict(SUITES["blender"], sourceRoots=[root]),
            ):
                with self.assertRaisesRegex(ValueError, "invalid source root"):
                    validate_catalog()

    def test_catalog_rejects_undeclared_product_reads(self):
        root = "dist/tests/react/gui-root.test.js"
        with patch.dict(TEST_INPUTS[root], builds=["typescript"]):
            with self.assertRaisesRegex(
                ValueError,
                f"test:{root} reads target/browser-build/render .* "
                "without build:browser:render",
            ):
                validate_catalog()

    def test_catalog_rejects_partition_exclusions_nothing_names(self):
        entry = next(
            entry
            for entry in SUITES["presentation"]["commands"]
            if entry["name"] == "webgl"
        )
        with patch.dict(
            entry, partitionExcludes=[*entry["partitionExcludes"], "target/gui-motion"]
        ):
            with self.assertRaisesRegex(
                ValueError, "excludes target/gui-motion, which it never names"
            ):
                validate_catalog()

    def test_focused_native_checks_do_not_need_node_or_browser(self):
        selected = plan(
            "check",
            "workspace",
            "clippy-default",
            "clippy-all-features",
            "--suite",
            "runner",
        )
        self.assertEqual(
            {r for task in selected.tasks for r in task.requirements}, {"rust"}
        )
        self.assertEqual(selected.coverage, "focused")

    def test_full_flag_does_not_silently_omit_gles(self):
        selected = plan("regression", "--full")
        self.assertEqual(selected.coverage, "full")
        self.assertTrue(any("gles" in task.requirements for task in selected.tasks))

    def test_browser_and_rendering_groups_do_not_select_native_canvas(self):
        for group in ("browser", "rendering"):
            with self.subTest(group=group):
                selected = plan("regression", "--group", group)
                ids = {task.id for task in selected.tasks}
                self.assertNotIn("build:gles-host", ids)
                self.assertNotIn("test:canvas:controller-gles", ids)
                self.assertFalse(
                    any("gles" in task.requirements for task in selected.tasks)
                )
                if group == "rendering":
                    self.assertTrue(
                        {
                            "test:canvas:lifecycle",
                            "test:canvas:dom",
                            "test:canvas:controller-webgl",
                        }.issubset(ids)
                    )

    def test_canvas_commands_replace_obsolete_dist_inputs(self):
        selected = plan("regression", "--suite", "canvas")
        tasks = {task.id: task for task in selected.tasks}
        self.assertEqual(
            tasks["test:canvas:dom"].command[1:],
            (
                "--test",
                "target/canvas-build/canvas.test.js",
                "target/canvas-build/dpi.test.js",
            ),
        )
        self.assertEqual(
            tasks["test:canvas:dom"].dependencies,
            (
                "build:canvas-fixtures",
                "build:browser:headless",
                "build:browser:render",
                "build:browser:render-instrumentation",
            ),
        )
        for name in ("canvas", "dpi"):
            path = f"dist/tests/render/{name}.test.js"
            self.assertNotIn(path, TEST_INPUTS)
            self.assertNotIn(f"test:{path}", tasks)
        self.assertNotIn("build:typescript", tasks)

    def test_canvas_controller_backends_have_separate_prerequisites(self):
        for backend in ("webgl", "gles"):
            with self.subTest(backend=backend):
                selected = plan(
                    "regression",
                    "--only",
                    f"test:canvas:controller-{backend}",
                    "--egl-dir",
                    "/configured/egl",
                )
                ids = {task.id for task in selected.tasks}
                requirements = {
                    name for task in selected.tasks for name in task.requirements
                }
                task = selected.tasks[-1]
                self.assertEqual(
                    task.command[1], "target/canvas-build/canvas-controller.test.js"
                )
                self.assertEqual(task.command[2:4], ("--backend", backend))
                self.assertIn("browser", requirements)
                self.assertIn("build:canvas-fixtures", ids)
                # The controller simulates context loss, a testing control.
                if backend == "gles":
                    self.assertIn("gles", task.requirements)
                    self.assertIn("build:gles-host-instrumentation", ids)
                    self.assertNotIn("build:browser:render-instrumentation", ids)
                    self.assertEqual(task.command[4:], ("--egl-dir", "/configured/egl"))
                else:
                    self.assertNotIn("gles", requirements)
                    self.assertNotIn("build:gles-host-instrumentation", ids)
                    self.assertIn("build:browser:render-instrumentation", ids)
                    self.assertEqual(task.command[4:], ())

    def test_canvas_native_egl_selection_and_preflight(self):
        with patch.dict(os.environ, IPP_EGL_LIBRARY_DIR="/environment/egl"):
            for flags, expected in (
                ((), "/environment/egl"),
                (("--egl-dir", "/explicit/egl"), "/explicit/egl"),
            ):
                with self.subTest(flags=flags):
                    selected = plan(
                        "regression", "--only", "test:canvas:controller-gles", *flags
                    )
                    self.assertEqual(
                        selected.tasks[-1].command[-2:], ("--egl-dir", expected)
                    )
        with patch.dict(os.environ):
            os.environ.pop("IPP_EGL_LIBRARY_DIR", None)
            args = parser().parse_args(
                ["regression", "--only", "test:canvas:controller-gles"]
            )
            self.assertIsNone(args.egl_dir)
            selected = make_plan(args)
            self.assertIn("gles", selected.tasks[-1].requirements)
            requirement = next(
                result
                for result in inspect_environment({"gles"}, args.egl_dir)
                if result.name == "gles"
            )
            self.assertFalse(requirement.ready)
            self.assertIn("--egl-dir", requirement.remedy)

    def test_canvas_variants_remain_reachable_without_duplicate_runs(self):
        both = {"test:canvas:controller-webgl", "test:canvas:controller-gles"}
        self.assertTrue(both.issubset(suite_ids(["canvas"])))
        self.assertIn("test:canvas:controller-gles", regression_group_ids(["gles"]))
        self.assertNotIn("test:canvas:controller-webgl", regression_group_ids(["gles"]))
        for arguments in (
            ("--suite", "canvas"),
            ("--group", "rendering", "--group", "gles", "--suite", "canvas"),
        ):
            with self.subTest(arguments=arguments):
                ids = [task.id for task in plan("regression", *arguments).tasks]
                for name in both | {
                    "build:canvas-fixtures",
                    "build:gles-host-instrumentation",
                }:
                    self.assertEqual(ids.count(name), 1)

    def test_default_core_keeps_real_native_coverage_without_optional_environments(
        self,
    ):
        selected = plan("regression")
        ids = {task.id for task in selected.tasks}
        self.assertEqual(selected.coverage, "core")
        self.assertTrue(
            {
                "check:repository",
                "check:workspace",
                "check:typecheck",
                "check:clippy-default",
                "test:rust:default",
                "test:runner:pipeline",
                "build:native",
                "build:client",
                "build:typescript",
                "test:dist/tests/integration/integration.test.js",
            }.issubset(ids)
        )
        self.assertTrue(set(suite_ids(["client"])).issubset(ids))
        requirements = {r for task in selected.tasks for r in task.requirements}
        self.assertFalse(
            requirements & {"wasm", "browser", "blender", "blender-wheels", "gles"}
        )
        self.assertNotIn("test:rust:all-features", ids)
        self.assertFalse(ids & set(suite_ids(["scaling"])))

    def test_groups_compose_with_core_and_share_prerequisites(self):
        selected = plan(
            "regression", "--group", "gui", "--group", "rendering", "--group", "gui"
        )
        ids = [task.id for task in selected.tasks]
        self.assertEqual(selected.coverage, "core+gui+rendering")
        self.assertEqual(len(ids), len(set(ids)))
        self.assertTrue(set(plan("regression").requested).issubset(ids))
        self.assertTrue(
            set(suite_ids(["gui", "surfaces", "retained-gui", "render"])).issubset(ids)
        )
        self.assertEqual(ids.count("build:typescript"), 1)
        self.assertNotIn("test:rust:all-features", ids)
        self.assertNotIn("test:dist/tests/integration/scaling.test.js", ids)

    def test_presentation_profiles_keep_browser_independent_of_gles(self):
        webgl = plan("regression", "--only", "test:presentation:webgl")
        self.assertFalse(any("gles" in task.requirements for task in webgl.tasks))
        for group in ("browser", "rendering"):
            selected = set(regression_group_ids([group]))
            self.assertIn("test:presentation:webgl", selected)
            self.assertIn("test:presentation:webgl-production", selected)
            self.assertNotIn("test:presentation:native-gles", selected)
        self.assertIn("test:presentation:native-gles", regression_group_ids(["gles"]))
        self.assertEqual(
            set(suite_ids(["presentation"])),
            {
                "test:presentation:webgl",
                "test:presentation:webgl-production",
                "test:presentation:native-gles",
                "test:presentation:diagnostics",
                "test:presentation:host-wire",
            },
        )

    def test_manifest_entries_build_the_current_host_wire_driver(self):
        for entry in (
            "test:lifecycle:manifest-codec",
            "test:crates/ipp-protocol/tests/manifest-client.mjs",
        ):
            with self.subTest(entry=entry):
                selected = plan("regression", "--only", entry)
                self.assertEqual(
                    {task.id for task in selected.tasks},
                    {"build:client", "build:presentation-wire-host", entry},
                )
                requirements = {
                    name for task in selected.tasks for name in task.requirements
                }
                self.assertEqual(requirements, {"node", "npm", "rust"})
        self.assertIn("check:typecheck", plan("regression").requested)

    def test_groups_keep_all_full_checks_reachable_and_benchmarks_separate(self):
        tasks = catalog("/example/egl")
        grouped = regression_group_ids(list(REGRESSION_GROUPS))
        covered = set(regression_ids(tasks)) | set(grouped)
        self.assertEqual(covered, set(regression_ids(tasks, full=True)))
        for group in REGRESSION_GROUPS:
            with self.subTest(group=group):
                selected = plan("regression", "--group", group)
                self.assertFalse(
                    any(task.id.startswith("benchmark:") for task in selected.tasks)
                )
        selected = plan("regression", "--group", "gles")
        self.assertTrue(any("gles" in task.requirements for task in selected.tasks))

    def test_new_checks_need_an_on_demand_group_or_core_assignment(self):
        tasks = catalog()
        tasks["check:new"] = Task(
            "check:new", "New check", (sys.executable, "-c", "pass")
        )
        with patch("pipeline.operations.catalog", return_value=tasks):
            with self.assertRaisesRegex(ValueError, "Assign checks/suites.*check:new"):
                validate_catalog()

    def test_focused_selections_and_explicit_full_flag_are_not_ignored(self):
        focused = plan("regression", "--suite", "runner")
        self.assertEqual(focused.requested, ("test:runner:pipeline",))
        self.assertEqual(focused.coverage, "partial-regression")
        selected = plan("regression", "--full", "--suite", "browser")
        ids = {task.id for task in selected.tasks}
        self.assertIn("test:rust:default", ids)
        self.assertTrue(set(suite_ids(["browser"])).issubset(ids))
        self.assertEqual(selected.coverage, "full")
        self.assertEqual(
            set(selected.requested), set(regression_ids(catalog(), full=True))
        )
        grouped = plan("regression", "--group", "browser", "--suite", "runner")
        self.assertIn("test:rust:default", grouped.requested)
        listed = list_selections(parser().parse_args(["regression", "--list"]))
        self.assertEqual(listed["default"], "core")
        self.assertEqual(set(listed["groups"]), set(REGRESSION_GROUPS))
        self.assertIn("--full", listed)
        self.assertNotIn("profiles", listed)

    def test_retry_groups_only_add_selected_work_and_preserve_egl_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "summary.json"
            report.write_text(
                json.dumps(
                    {
                        "version": 2,
                        "root": str(ROOT),
                        "coverage": "full",
                        "eglDirectory": "/example/egl",
                        "steps": [
                            {"id": "check:catalog", "status": "passed"},
                            {"id": "check:repository", "status": "failed"},
                        ],
                    }
                )
            )
            with patch.dict(os.environ):
                # The recorded directory must win without an ambient default.
                os.environ.pop("IPP_EGL_LIBRARY_DIR", None)
                for args in (
                    ("retry", str(report)),
                    ("regression", "--retry", str(report)),
                ):
                    with self.subTest(args=args):
                        selected = plan(*args, "--group", "gles")
                        self.assertEqual(selected.coverage, "partial-regression")
                        self.assertIn("check:repository", selected.requested)
                        self.assertNotIn("check:catalog", selected.requested)
                        self.assertNotIn("test:rust:default", selected.requested)
                        gles = next(
                            t for t in selected.tasks if t.id == "check:gles-particles"
                        )
                        self.assertIn("/example/egl", gles.command)
                        canvas = next(
                            task
                            for task in selected.tasks
                            if task.id == "test:canvas:controller-gles"
                        )
                        self.assertEqual(
                            canvas.command[-2:], ("--egl-dir", "/example/egl")
                        )
            with self.assertRaisesRegex(ValueError, "retry cannot select --full"):
                plan("regression", "--retry", str(report), "--full")

    def test_unknown_steps_cycles_and_invalid_outputs_fail_before_execution(self):
        with self.assertRaisesRegex(ValueError, "Unknown task"):
            plan("regression", "--only", "check:native-default")
        with self.assertRaisesRegex(ValueError, "Unknown suite"):
            plan("test", "camera")
        with self.assertRaisesRegex(ValueError, "Name focused suites"):
            plan("test")
        command = (sys.executable, "-c", "pass")
        with self.assertRaisesRegex(ValueError, "cycle"):
            select(
                {
                    "a": Task("a", "a", command, ("b",)),
                    "b": Task("b", "b", command, ("a",)),
                },
                ["a"],
            )
        with self.assertRaisesRegex(ValueError, "within the workspace"):
            Task("a", "a", command, outputs=("../outside",))
        with self.assertRaisesRegex(ValueError, "Overlapping products"):
            select(
                {
                    "a": Task("a", "a", command, outputs=("target/shared",)),
                    "b": Task("b", "b", command, outputs=("target/shared/child",)),
                },
                ["a"],
            )

    def test_retry_of_completed_report_never_selects_full_regression(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "summary.json"
            report.write_text(
                json.dumps(
                    {
                        "version": 2,
                        "root": str(ROOT),
                        "steps": [{"id": "check:catalog", "status": "passed"}],
                    }
                )
            )
            for args in (
                ("retry", str(report)),
                ("regression", "--retry", str(report)),
            ):
                selected = plan(*args)
                self.assertEqual(selected.tasks, ())
                self.assertEqual(selected.coverage, "partial-regression")

    def test_planning_requires_no_installed_tools_and_ignores_caller_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "tools/ipp.py"),
                    "build",
                    "gallery",
                    "--plan",
                    "--json",
                ],
                cwd=directory,
                env={**os.environ, "PATH": ""},
                capture_output=True,
                text=True,
            )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["requested"], ["build:gallery"])

    def test_changed_selection_is_explicit_about_unknown_coverage(self):
        ids, notes = affected(["README.md"], [])
        self.assertIn("check:format-md", ids)
        self.assertFalse(any(id_.startswith("test:") for id_ in ids))
        with self.assertRaisesRegex(ValueError, "--suite"):
            affected(["crates/ipp-core/src/world/storage.rs"], [])
        ids, notes = affected(["crates/ipp-core/src/world/storage.rs"], ["worlds"])
        self.assertTrue(any("Explicit suites" in note for note in notes))


class ExecutionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ipp-pipeline-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        (self.root / ".gitignore").write_text("target/\n")
        subprocess.run(["git", "add", ".gitignore"], cwd=self.root, check=True)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Pipeline test",
                "-c",
                "user.email=pipeline@example.invalid",
                "commit",
                "-qm",
                "Test fixture",
            ],
            cwd=self.root,
            check=True,
        )

    def command(self, id_, source, dependencies=(), **options):
        return Task(id_, id_, (sys.executable, "-c", source), dependencies, **options)

    def run_tasks(self, tasks, **options):
        selected = Plan("test", tuple(task.id for task in tasks), tuple(tasks))
        with redirect_stderr(io.StringIO()):
            return run_plan(selected, root=self.root, preflight=False, **options)

    def test_failures_block_dependents_but_retain_independent_evidence(self):
        tasks = [
            self.command(
                "build",
                "from pathlib import Path; Path('target/product').mkdir(parents=True); Path('target/product/file').write_text('built')",
                outputs=("target/product",),
            ),
            self.command(
                "failure",
                "import sys; print('useful failure'); sys.exit(7)",
                ("build",),
            ),
            self.command("blocked", "raise Exception('must not run')", ("failure",)),
            self.command("independent", "print('still ran')", ("build",)),
            Task("missing", "missing", (str(self.root / "missing"),)),
        ]
        report = self.run_tasks(tasks)
        self.assertEqual(
            [step["status"] for step in report["steps"]],
            ["passed", "failed", "blocked", "passed", "failed"],
        )
        self.assertEqual(report["steps"][1]["exitCode"], 7)
        self.assertIn("useful failure", Path(report["steps"][1]["log"]).read_text())
        manifest = json.loads(Path(report["steps"][0]["manifest"]).read_text())
        self.assertEqual(manifest["artifacts"][0]["path"], "target/product/file")
        saved = json.loads((Path(report["directory"]) / "summary.json").read_text())
        self.assertEqual(saved["status"], "failed")
        retry, _ = retry_ids(Path(report["directory"]) / "summary.json", self.root)
        self.assertEqual(retry, ["failure", "blocked", "missing"])

    def test_missing_declared_output_fails_even_when_process_succeeds(self):
        report = self.run_tasks(
            [self.command("build", "pass", outputs=("target/missing",))]
        )
        self.assertEqual(report["status"], "failed")
        self.assertIn("Declared output", report["steps"][0]["error"])

    def test_preflight_failure_runs_no_builds(self):
        task = self.command(
            "build", "from pathlib import Path; Path('unexpected').touch()"
        )
        with patch(
            "pipeline.runner.inspect",
            return_value=[Requirement("browser", False, "missing", "setup browser")],
        ):
            with redirect_stderr(io.StringIO()):
                report = run_plan(Plan("test", (task.id,), (task,)), root=self.root)
        self.assertEqual(report["status"], "environment_failed")
        self.assertFalse((self.root / "unexpected").exists())
        self.assertEqual(report["steps"][0]["status"], "not_run")

    def test_content_identity_distinguishes_two_edits_of_the_same_file(self):
        source = self.root / "source.py"
        source.write_text("first")
        first = source_identity(self.root)
        source.write_text("second")
        second = source_identity(self.root)
        self.assertEqual(first["revision"], second["revision"])
        self.assertNotEqual(first["sourceSha256"], second["sourceSha256"])

    def test_evidence_directories_are_unique(self):
        one = self.run_tasks([self.command("ok", "pass")])
        two = self.run_tasks([self.command("ok", "pass")])
        self.assertNotEqual(one["directory"], two["directory"])
        self.assertTrue((Path(one["directory"]) / "summary.json").is_file())

    def test_exited_launcher_leaves_no_listening_descendants(self):
        descendant = "from pathlib import Path; import socket,time; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); Path('port').write_text(str(s.getsockname()[1])); time.sleep(60)"
        parent = f"from pathlib import Path; import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{descendant!r}], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL);\nwhile not Path('port').exists(): time.sleep(0.02)"
        report = self.run_tasks([self.command("launcher", parent, timeout=10)])
        self.assertEqual(report["status"], "passed")
        port = int((self.root / "port").read_text())
        with socket.socket() as connection:
            connection.settimeout(1)
            self.assertNotEqual(connection.connect_ex(("127.0.0.1", port)), 0)

    def test_environment_probe_is_cancellable(self):
        ready = self.root / "probe-ready"
        command = [
            sys.executable,
            "-c",
            f"from pathlib import Path; import time; Path({str(ready)!r}).touch(); time.sleep(60)",
        ]
        cancel = threading.Event()

        def stop_when_ready():
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not ready.exists():
                time.sleep(0.02)
            cancel.set()

        stopper = threading.Thread(target=stop_when_ready)
        stopper.start()
        try:
            with self.assertRaisesRegex(ValueError, "cancelled"):
                probe(command, cancel=cancel)
        finally:
            cancel.set()
            stopper.join(timeout=6)
        self.assertTrue(ready.exists())

    def test_workspace_lock_is_released_and_prevents_other_writers(self):
        script = "import sys; sys.path.insert(0, sys.argv[1]); from pathlib import Path; from pipeline.artifacts import workspace_lock;\nwith workspace_lock(Path(sys.argv[2])): print('locked')"
        command = [sys.executable, "-c", script, str(ROOT / "tools"), str(self.root)]
        with workspace_lock(self.root):
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Another pipeline", result.stderr)
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
