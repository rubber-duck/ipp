"""Catalog and registry structure, CI wiring, and suite and task validation."""

from pathlib import Path
import sys
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
from pipeline.catalog_validation import validate_catalog
from pipeline.model import ROOT, Task, select
from pipeline.registries import GLES_CHECKS, SUITES, TEST_INPUTS
from pipeline.selection import affected
from support import plan


class CatalogTests(unittest.TestCase):
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

    def test_native_retained_gui_check_drives_its_client_on_the_selected_egl(self):
        task = catalog("/validation/egl")["check:gles-retained-gui"]
        self.assertEqual(
            task.command[1:],
            ("target/surface-gui-build/retained-gui.native.test.js", "/validation/egl"),
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

    def test_catalog_rejects_missing_or_escaping_source_roots(self):
        for root in (
            "../outside/",
            "tests",
            "missing-directory/",
            "tests/rendering/pages/missing.tsx",
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
        root = "dist/tests/react/gui-authoring/gui-root.test.js"
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
        with patch("pipeline.catalog_validation.catalog", return_value=tasks):
            with self.assertRaisesRegex(ValueError, "Assign checks/suites.*check:new"):
                validate_catalog()

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


if __name__ == "__main__":
    unittest.main()
