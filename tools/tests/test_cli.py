"""Command-line parsing, planning, selection listings and plan output."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from pipeline.catalog import (
    REGRESSION_GROUPS,
    catalog,
    regression_group_ids,
    regression_ids,
    suite_ids,
)
from pipeline.cli import list_selections, make_plan, parser
from pipeline.environment import inspect as inspect_environment
from pipeline.model import ROOT, select
from pipeline.registries import TEST_INPUTS
from support import plan


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

    def test_chart_data_uses_release_only_benchmark_products(self):
        native = plan(
            "benchmark", "native", "--scene", "chart-data", "--egl-dir", "/example/egl"
        )
        native_ids = [task.id for task in native.tasks]
        self.assertIn("build:performance-chart-data", native_ids)
        self.assertNotIn("build:gles-host", native_ids)
        self.assertEqual(native_ids[-1], "benchmark:chart-data")
        for kind in ("data", "chart"):
            for mode, flags in (("timing", []), ("allocations", ["--instrumented"])):
                selected = plan(
                    "benchmark",
                    "native",
                    "--scene",
                    "chart-data",
                    "--diagnostic",
                    kind,
                    "--build-only",
                    *flags,
                )
                self.assertEqual(
                    [task.id for task in selected.tasks],
                    [f"build:performance-chart-{kind}-{mode}"],
                )
                self.assertEqual(
                    selected.tasks[0].outputs,
                    (f"target/performance-build/chart-{kind}-{mode}",),
                )
        tasks = catalog("/example/egl")
        for full in (False, True):
            self.assertFalse(
                any(
                    "performance-chart" in task.id
                    or task.id.startswith("benchmark:chart")
                    for task in select(tasks, regression_ids(tasks, full=full))
                )
            )

    def test_chart_data_rejects_invalid_sampling_and_arrangements(self):
        base = (
            "benchmark",
            "native",
            "--scene",
            "chart-data",
            "--egl-dir",
            "/example/egl",
        )
        for flags in (
            ("--samples", "0"),
            ("--samples", "65"),
            ("--repetitions", "0"),
            ("--repetitions", "9"),
            ("--warmup", "-1"),
            ("--warmup", "17"),
            ("--instrumented",),
        ):
            with self.subTest(flags=flags), self.assertRaises(ValueError):
                plan(*base, *flags)
        with self.assertRaises(ValueError):
            plan("benchmark", "browser", "--scene", "chart-data")
        with patch.dict(os.environ):
            os.environ.pop("IPP_EGL_LIBRARY_DIR", None)
            with self.assertRaises(ValueError):
                plan("benchmark", "native", "--scene", "chart-data")
        with patch.dict(os.environ, {"IPP_EGL_LIBRARY_DIR": "/example/egl"}):
            inherited = plan("benchmark", "native", "--scene", "chart-data")
            self.assertEqual(inherited.tasks[-1].id, "benchmark:chart-data")

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
        self.assertIn("build:performance-gui-native", native.dependencies)
        self.assertIn("--host-build", native.command)
        diagnostics = plan(
            "benchmark",
            "native",
            "--scene",
            "gui-stress",
            "--egl-dir",
            "/lib64",
            "--instrumented",
        ).tasks[-1]
        self.assertIn(
            "build:performance-gui-native-instrumented", diagnostics.dependencies
        )
        self.assertIn("--instrumented-diagnostics", diagnostics.command)
        for flags in (("--frames", "4"), ("--group", "32"), ("--surface-cache",)):
            with self.subTest(flags=flags), self.assertRaises(ValueError):
                plan("benchmark", "browser", "--scene", "gui-stress", *flags)

    def test_trace_capacity_and_instrumented_products(self):
        with self.assertRaises(ValueError):
            plan("trace", "browser", "--max-events", "0")
        with (
            patch.dict(os.environ, {"IPP_EGL_LIBRARY_DIR": ""}),
            self.assertRaises(ValueError),
        ):
            plan("trace", "native", "--max-events", "64")
        for backend in ("browser", "native"):
            args = ["trace", backend, "--max-events", "64", "--software"]
            if backend == "native":
                args += ["--egl-dir", "/usr/lib64"]
            selected = plan(*args)
            names = {task.id for task in selected.tasks}
            self.assertIn("build:browser:render-instrumentation", names)
            self.assertIn("trace:gallery-gui", names)
            self.assertIn("64", selected.tasks[-1].command)
            self.assertEqual(
                "build:gles-host-instrumentation" in names, backend == "native"
            )

    def test_gui_software_selection_is_diagnostic_only(self):
        arguments = (
            "benchmark",
            "native",
            "--scene",
            "gui-stress",
            "--egl-dir",
            "/lib64",
            "--software",
        )
        with self.assertRaisesRegex(ValueError, "ordinary timing requires hardware"):
            plan(*arguments)
        diagnostic = plan(*arguments, "--instrumented").tasks[-1]
        self.assertIn("--instrumented-diagnostics", diagnostic.command)
        self.assertIn(
            "build:performance-gui-native-instrumented", diagnostic.dependencies
        )
        with self.assertRaisesRegex(ValueError, "native GUI"):
            plan(
                "benchmark",
                "browser",
                "--scene",
                "gui-stress",
                "--software",
                "--instrumented",
            )

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
            path = f"dist/tests/rendering/canvas/{name}.test.js"
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
                "test:dist/tests/runtime/transport.native.test.js",
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
        self.assertNotIn("test:dist/tests/runtime/scaling.native.test.js", ids)

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


if __name__ == "__main__":
    unittest.main()
