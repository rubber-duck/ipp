"""Formatting selection across actual Git checkout and nested worktree boundaries."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.operations import format_source, source_files


class FormattingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="ipp-formatting-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "-q")
        (self.root / ".gitignore").write_text("target/\n")
        (self.root / "src").mkdir()
        for name in ("tracked.ts", "tracked.py", "tracked.md", "deleted.ts"):
            (self.root / "src" / name).write_text("unformatted fixture\n")
        self.git("add", ".")
        self.git(
            "-c",
            "user.name=Formatting test",
            "-c",
            "user.email=formatting@example.invalid",
            "commit",
            "-qm",
            "Formatting fixture",
        )
        # An arbitrary location, so the test cannot pass through a .claude ignore.
        self.nested = self.root / "src/other-checkout"
        self.git("worktree", "add", "--detach", str(self.nested), "HEAD")
        (self.nested / "biome.json").write_text('{"root":true}\n')
        (self.nested / "untracked.py").write_text("invalid nested Python (\n")
        (self.root / "src/deleted.ts").unlink()
        for name in ("new file.tsx", "new.pyi", "new.md"):
            (self.root / "src" / name).write_text("unformatted fixture\n")
        (self.root / "target").mkdir()
        (self.root / "target/ignored.ts").write_text("invalid generated TS (\n")
        self.root_patch = patch("pipeline.operations.ROOT", self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.root, check=True, capture_output=True, text=True
        )

    def test_discovery_includes_new_files_but_excludes_other_checkouts_and_output(self):
        expected = [
            "src/new file.tsx",
            "src/new.md",
            "src/new.pyi",
            "src/tracked.md",
            "src/tracked.py",
            "src/tracked.ts",
        ]
        self.assertEqual(source_files([]), [".gitignore", *expected])
        self.assertEqual(source_files(["src"]), expected)
        self.assertEqual(source_files([str(self.root / "src")]), expected)
        self.assertEqual(source_files(["src/other-checkout"]), [])

    def test_linked_worktree_selects_its_own_files(self):
        with patch("pipeline.operations.ROOT", self.nested):
            files = source_files([])
        self.assertIn("src/deleted.ts", files)
        self.assertIn("untracked.py", files)
        self.assertNotIn("src/new file.tsx", files)
        self.assertFalse(any(name.startswith("../") for name in files))

    def test_formatters_receive_only_current_source_files_for_whole_or_scoped_runs(
        self,
    ):
        expected = {
            "js": ["src/new file.tsx", "src/tracked.ts"],
            "python": ["src/new.pyi", "src/tracked.py"],
            "md": ["src/new.md", "src/tracked.md"],
        }
        for language, files in expected.items():
            for paths in ([], ["."], ["src"]):
                with self.subTest(language=language, paths=paths):
                    # Observe the subprocess boundary; discovery uses real Git.
                    with patch("pipeline.operations.run") as run:
                        format_source(language, "write", paths)
                    command = run.call_args.args[0]
                    self.assertEqual(command[-len(files) :], files)
                    self.assertNotIn(".", command)
                    self.assertNotIn("src", command)

    def test_empty_selection_never_falls_back_to_scanning_the_working_directory(self):
        for language in ("js", "python", "md"):
            for paths in (["src/other-checkout"], ["target"], ["src/deleted.ts"]):
                with self.subTest(language=language, paths=paths):
                    with patch("pipeline.operations.run") as run:
                        format_source(language, "check", paths)
                    run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
