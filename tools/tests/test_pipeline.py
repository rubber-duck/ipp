"""Public planning contracts and real subprocess/evidence lifecycle coverage."""

from contextlib import redirect_stderr
import io
import json
import os
from pathlib import Path
import signal
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
    SUITES,
    catalog,
    regression_ids,
    suite_ids,
)
from pipeline.cli import make_plan, parser
from pipeline.environment import Requirement, probe
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
                    ids.index("build:browser:render-expanded"),
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

    def test_performance_scene_stays_outside_regression(self):
        selected = plan("benchmark", "native", "--preset", "full", "--culling-views")
        ids = [task.id for task in selected.tasks]
        self.assertIn("build:performance-native", ids)
        self.assertIn("benchmark:scene", ids)
        retained = plan("benchmark", "browser", "--scene", "retained-gui")
        retained_ids = [task.id for task in retained.tasks]
        self.assertIn("benchmark:retained-gui", retained_ids)
        self.assertIn("build:browser:headless-gui", retained_ids)
        self.assertIn("build:browser:render-surfaces", retained_ids)
        self.assertNotIn("build:blender-fixtures", retained_ids)
        with self.assertRaises(ValueError):
            plan("benchmark", "native", "--scene", "retained-gui")
        for profile in ("repository", "native", "integration"):
            tasks = catalog("/example/egl")
            regression = select(tasks, regression_ids(tasks, profile))
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

    def test_every_suite_and_ci_selection_resolves(self):
        validate_catalog()
        tasks = catalog("/example/egl")
        full = select(tasks, regression_ids(tasks, "integration"))
        self.assertEqual(len(full), len({task.id for task in full}))
        ids = [task.id for task in full]
        for name in SUITES:
            self.assertTrue(set(suite_ids([name])).issubset(ids))
        self.assertIn("check:gles-particles", ids)
        self.assertIn("check:browser-identities", ids)
        self.assertIn("check:contracts:expanded", ids)

    def test_camera_profiles_and_shared_tests_are_precise(self):
        selected = plan("test", "cameras")
        distributions = {
            task.id.removeprefix("build:browser:")
            for task in selected.tasks
            if task.id.startswith("build:browser:")
        }
        self.assertEqual(distributions, {"headless", "render", "render-expanded"})
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
        self.assertIn("build:browser:render-expanded", ids)
        self.assertNotIn("build:gallery-fixtures", ids)
        self.assertNotIn("build:typescript", ids)
        self.assertEqual(sum(id_.startswith("build:browser:") for id_ in ids), 1)

    def test_typecheck_prepares_its_actual_target(self):
        ids = [task.id for task in plan("check", "typecheck").tasks]
        self.assertEqual(ids, ["build:native", "build:client", "check:typecheck"])

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
            (
                "crates/ipp-core/src/world/systems/render/surface_preparation_tests.rs",
                suite_ids(["gui"]),
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
            {"check:gles-spatial", "check:gles-textures", "check:gles-lighting"},
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

    def test_native_ci_does_not_need_node_or_browser(self):
        selected = plan("regression", "--profile", "native")
        self.assertEqual(
            {r for task in selected.tasks for r in task.requirements}, {"rust"}
        )
        self.assertEqual(selected.coverage, "native")

    def test_full_profile_does_not_silently_omit_gles(self):
        selected = plan("regression")
        self.assertEqual(selected.coverage, "integration")
        self.assertTrue(any("gles" in task.requirements for task in selected.tasks))

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

    def test_fail_fast_and_timeout_share_the_executor(self):
        report = self.run_tasks(
            [
                self.command("timeout", "import time; time.sleep(60)", timeout=1),
                self.command("later", "print('unexpected')"),
            ],
            fail_fast=True,
        )
        self.assertTrue(report["steps"][0]["timedOut"])
        self.assertEqual(report["steps"][1]["status"], "not_run")

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

    @unittest.skipIf(
        os.name == "nt",
        "POSIX signal-handler fixture; Windows uses job/process-tree termination",
    )
    def test_cancellation_terminates_descendants_and_records_unstarted_work(self):
        descendant = "from pathlib import Path; import signal,time; signal.signal(signal.SIGTERM, lambda *_: (Path('stopped').touch(), exit(0))); Path('ready').touch(); time.sleep(60)"
        parent = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{descendant!r}]); time.sleep(60)"
        cancel = threading.Event()

        def stop_when_ready():
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and not (self.root / "ready").exists():
                time.sleep(0.02)
            cancel.set()

        stopper = threading.Thread(target=stop_when_ready)
        stopper.start()
        try:
            report = self.run_tasks(
                [self.command("running", parent), self.command("later", "pass")],
                cancel=cancel,
            )
        finally:
            cancel.set()
            stopper.join(timeout=6)
        self.assertTrue((self.root / "stopped").exists())
        self.assertEqual(
            [step["status"] for step in report["steps"]], ["cancelled", "cancelled"]
        )


if __name__ == "__main__":
    unittest.main()
