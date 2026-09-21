"""Source ownership checks run on small real directory trees."""

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from repository_structure import source_imports, structure_errors


class StructureTests(unittest.TestCase):
    def test_quoted_examples_are_not_module_dependencies(self):
        source = """const example = `
import { x } from "../../tests/example.js";
`;
const quoted = "import('../../tests/quoted.js')";
import { value } from "./literal/*part*/module.js";
export type { Value } from "./types.js";
const loaded = import("./real.js");
"""
        self.assertEqual(
            source_imports(source),
            ["./literal/*part*/module.js", "./types.js", "./real.js"],
        )

    def check_tree(self, files):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, source in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source)
            return structure_errors(root, list(files))

    def test_rust_children_require_directory_module(self):
        errors = self.check_tree(
            {
                "crates/core/src/storage.rs": "mod pages;",
                "crates/core/src/storage/pages.rs": "",
            }
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("storage/mod.rs", errors[0])

    def test_standalone_test_and_production_submodules_are_allowed(self):
        self.assertEqual(
            self.check_tree(
                {
                    "crates/core/src/storage.rs": '#[cfg(test)] #[path = "storage_tests.rs"] mod tests;',
                    "crates/core/src/storage_tests.rs": "",
                    "crates/core/src/animation/mod.rs": "mod update;",
                    "crates/core/src/animation/update.rs": "",
                    "crates/core/src/geometry/mod.rs": "mod shapes;",
                    "crates/core/src/geometry/geometry_tests.rs": "",
                    "crates/core/src/geometry/shapes/mod.rs": "mod primitives;",
                    "crates/core/src/geometry/shapes/primitives.rs": "",
                }
            ),
            [],
        )

    def test_directory_for_a_single_test_is_rejected(self):
        errors = self.check_tree(
            {
                "crates/core/src/storage/mod.rs": "",
                "crates/core/src/storage/storage_tests.rs": "",
            }
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("sole test file", errors[0])

    def test_production_cannot_import_test_or_example_implementation(self):
        errors = self.check_tree(
            {
                "packages/client/src/client.ts": 'export { helper } from "../../../tests/helper.js";',
                "integrations/blender/client/adapter.ts": 'import type { Scene } from "../../../examples/viewer/types.js";',
                "examples/gallery/main.ts": 'const helper = await import("../../tests/helper.js");',
            }
        )
        self.assertEqual(len(errors), 3)

    def test_production_path_attribute_leaving_directory_is_rejected(self):
        errors = self.check_tree(
            {
                "crates/render/src/service.rs": '#[path = "impl/service_impl.rs"]\nmod service;\n',
                "crates/render/src/impl/service_impl.rs": "",
            }
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("#[path]", errors[0])
        self.assertIn("impl/service_impl.rs", errors[0])

    def test_parent_directory_path_attribute_is_rejected(self):
        errors = self.check_tree(
            {
                "crates/render/src/service.rs": '#[path = "../shared/service_impl.rs"]\nmod service;\n',
            }
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("#[path]", errors[0])

    def test_adjacent_tests_and_sibling_modules_are_allowed(self):
        self.assertEqual(
            self.check_tree(
                {
                    "crates/core/src/storage.rs": '#[cfg(test)]\n#[path = "storage_tests.rs"]\nmod tests;\n',
                    "crates/core/src/storage_tests.rs": "",
                    "crates/core/src/input.rs": '#[path = "text_edit.rs"]\nmod text_edit;\n',
                    "crates/core/src/text_edit.rs": "",
                    "crates/core/src/preparation.rs": '#[cfg(test)]\n#[path = "preparation_fixture.rs"]\nmod fixture;\n',
                    "crates/core/src/preparation_fixture.rs": "",
                }
            ),
            [],
        )

    def test_path_attribute_in_comments_strings_and_chars_is_ignored(self):
        self.assertEqual(
            self.check_tree(
                {
                    "crates/core/src/storage.rs": (
                        '// #[path = "../elsewhere.rs"]\n'
                        '/* #[path = "../block.rs"] */\n'
                        '/* outer /* #[path = "../nested.rs"] */ still comment */\n'
                        'const EXAMPLE: &str = "#[path = \\"../string.rs\\"]";\n'
                        'const RAW: &str = r#"#[path = "../raw.rs"]"#;\n'
                        "const MARKER: char = '#';\n"
                        "mod storage;\n"
                    ),
                }
            ),
            [],
        )

    def test_path_attribute_outside_src_is_ignored(self):
        self.assertEqual(
            self.check_tree(
                {
                    "crates/server/examples/host.rs": '#[path = "../../render/examples/smoke/egl.rs"]\nmod egl;\n',
                }
            ),
            [],
        )

    def test_path_attribute_scan_has_no_length_limits(self):
        padding = "// filler\n" * 20000
        errors = self.check_tree(
            {
                "crates/render/src/service.rs": (
                    padding
                    + "fn late() {}\n"
                    + padding
                    + '#[path = "impl/service_impl.rs"]\nmod service;\n'
                ),
                "crates/render/src/impl/service_impl.rs": "",
            }
        )
        self.assertEqual(len(errors), 1)
        self.assertIn("#[path]", errors[0])

    def test_tests_may_use_real_examples_and_commented_imports_are_ignored(self):
        self.assertEqual(
            self.check_tree(
                {
                    "tests/render/fixture.ts": 'import { createMesh } from "../../examples/gallery/mesh.js";',
                    "examples/gallery/mesh.ts": '/* import { x } from "../../tests/x.js"; */\nexport const mesh = 1;',
                    "packages/client/src/client.ts": '// import { x } from "../../../tests/x.js";\nimport type { Value } from "./types.js";',
                }
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()
