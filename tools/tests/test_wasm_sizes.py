"""Framing, attribution and measurement contracts for real distribution reports."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from measure_artifacts import artifact, distribution
from pipeline.cli import make_plan, parser
from wasm_sizes import analyze_wasm


HEADER = b"\0asm\1\0\0\0"


def section(kind, payload):
    assert len(payload) < 128
    return bytes([kind, len(payload)]) + payload


class WasmSizeTests(unittest.TestCase):
    def test_named_import_offset_and_accounting(self):
        imports = section(2, b"\1\1m\1f\0\0")
        functions = section(3, b"\2\0\0")
        code = section(10, b"\2\2\0\x0b\3\0\x01\x0b")
        name_map = b"\1\1\x0bcrate::work"
        names = section(0, b"\4name" + section(1, name_map))
        data = HEADER + imports + functions + code + names
        report = analyze_wasm(data)
        self.assertEqual(report["attribution"]["named_body_bytes"], 2)
        self.assertEqual(report["attribution"]["unknown_body_bytes"], 3)
        self.assertEqual(report["attribution"]["groups"], {"crate": 2})
        self.assertEqual(
            sum(s["payload_bytes"] + s["framing_bytes"] for s in report["sections"])
            + 8,
            len(data),
        )
        self.assertEqual(
            report["code_payload_bytes"]
            + report["data_payload_bytes"]
            + report["other_and_framing_bytes"],
            len(data),
        )

    def test_export_aliases_count_a_body_once(self):
        exports = section(7, b"\2\1b\0\0\1a\0\0")
        result = analyze_wasm(
            HEADER + section(3, b"\1\0") + exports + section(10, b"\1\2\0\x0b")
        )
        attribution = result["attribution"]
        self.assertEqual(attribution["named_body_bytes"], 2)
        self.assertEqual(attribution["unknown_body_bytes"], 0)
        self.assertEqual(attribution["symbols"][0]["export_aliases"], ["a", "b"])
        self.assertEqual(attribution["symbols"][0]["name_source"], "export")

    def test_shader_overlap_is_counted_once(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = tuple(Path(directory) / name for name in ("a", "b"))
            paths[0].write_bytes(b"abcdef")
            paths[1].write_bytes(b"cdef")
            result = analyze_wasm(HEADER + section(11, b"abcdefabcdef"), paths)
            self.assertEqual(result["embedded_shaders"]["matched_bytes"], 12)
            self.assertEqual(result["embedded_shaders"]["data_remainder_bytes"], 0)

    def test_malformed_modules_fail(self):
        cases = [
            b"",
            HEADER[:-1],
            b"bad!" + HEADER[4:],
            HEADER + b"\x0a\xff",
            HEADER + b"\x01\x80\x80\x80\x80\x10",
            HEADER + section(10, b"\1\5\0"),
            HEADER + section(3, b"\1\0"),
            HEADER + section(1, b"") * 2,
        ]
        for data in cases:
            with self.subTest(data=data), self.assertRaises(ValueError):
                analyze_wasm(data)

    def test_stripped_names_are_explicitly_unknown(self):
        result = analyze_wasm(HEADER + section(3, b"\1\0") + section(10, b"\1\2\0\x0b"))
        self.assertEqual(result["attribution"]["unknown_body_bytes"], 2)
        self.assertEqual(result["attribution"]["symbols"], [])

    def test_missing_artifact_and_profile_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(FileNotFoundError):
                artifact(root / "absent")
            product = root / "target/browser-build/render"
            product.mkdir(parents=True)
            (product / "build-report.json").write_text(
                json.dumps({"configuration": "headless", "features": []})
            )
            with self.assertRaises(ValueError):
                distribution(root, "render", ["render"])

    def test_distribution_hashes_js_inventory_and_notices(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            product = root / "target/browser-build/render"
            product.mkdir(parents=True)
            wasm = product / "runtime.wasm"
            wasm.write_bytes(HEADER)
            js = product / "worker.js"
            js.write_bytes(b"export {};\n")
            notice = product / "SLUG-NOTICE"
            notice.write_bytes(b"license")
            records = []
            for path in (wasm, js, notice):
                record = artifact(path)
                record["path"] = str(path.relative_to(root))
                records.append(record)
            (product / "build-report.json").write_text(
                json.dumps(
                    {
                        "configuration": "render",
                        "features": ["render"],
                        "artifacts": records,
                    }
                )
            )
            result = distribution(root, "render", ["render"])
            self.assertEqual(
                result["totals"]["raw_bytes"], len(HEADER) + len(js.read_bytes())
            )
            self.assertEqual(len(result["ancillary_artifacts"]), 1)
            js.write_bytes(b"modified")
            with self.assertRaisesRegex(ValueError, "differs from verified"):
                distribution(root, "render", ["render"])
            js.write_bytes(b"export {};\n")
            (product / "extra.js").write_bytes(b"extra")
            with self.assertRaisesRegex(ValueError, "inventory mismatch"):
                distribution(root, "render", ["render"])

    def test_profile_planning_and_explicit_measure_compatibility(self):
        planned = make_plan(
            parser().parse_args(
                ["measure", "--profile", "render", "--profile", "render"]
            )
        )
        self.assertEqual(planned.tasks[-1].dependencies, ("build:browser:render",))
        self.assertEqual(
            make_plan(parser().parse_args(["measure", "file.wasm"]))
            .tasks[-1]
            .dependencies,
            (),
        )
        with self.assertRaises(ValueError):
            make_plan(parser().parse_args(["measure"]))


if __name__ == "__main__":
    unittest.main()
