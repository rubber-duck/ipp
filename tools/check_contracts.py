#!/usr/bin/env python3
"""Execute native/WASM exports and verify reproducible, feature-matched clients."""

import json
from pathlib import Path
import sys
import subprocess

from pipeline.catalog import PROFILES
from pipeline.processes import node, run as execute

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "target/integration-artifacts/contracts"


def run(*args, output=None):
    command = [node() if part == "node" else part for part in args]
    execute(command, output=output)


def export(example, features, output):
    run(
        "cargo",
        "run",
        "--quiet",
        "-p",
        "ipp-protocol",
        "--locked",
        "--no-default-features",
        "--features",
        features,
        "--example",
        example,
        output=output,
    )


def main(selected_profiles=None):
    reports = {}
    generated = []
    configurations = PROFILES["contracts"]
    available = tuple(configurations)
    selected = (
        tuple(selected_profiles if selected_profiles is not None else sys.argv[1:])
        or available
    )
    if len(set(selected)) != len(selected) or any(
        name not in available for name in selected
    ):
        raise ValueError(f"Select unique configurations from {', '.join(available)}")
    for configuration in selected:
        directory = ARTIFACTS / configuration
        directory.mkdir(parents=True, exist_ok=True)
        features = configurations[configuration]
        protocol_features = ",".join(["schema-export", *features])
        wasm_features = protocol_features
        native = directory / "native.contract"
        fixture = directory / "native.fixture"
        repeated = directory / "repeated.contract"
        export("export_contract", protocol_features, native)
        export("export_contract", protocol_features, repeated)
        if native.read_bytes() != repeated.read_bytes():
            raise ValueError(f"{configuration}: native export is not reproducible")
        export("export_fixture", protocol_features, fixture)
        run(
            "cargo",
            "build",
            "-p",
            "ipp-wasm",
            "--locked",
            "--no-default-features",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            wasm_features,
        )
        run(
            "node",
            "tools/ipp-schema-gen/tests/target-contract.mjs",
            str(native),
            str(fixture),
            "target/wasm32-unknown-unknown/debug/ipp_wasm.wasm",
            str(directory),
        )
        report = json.loads((directory / "target-report.json").read_text())
        names = [component["name"] for component in report["native"]["components"]]
        expected = [
            "Scalar",
            "LinearDriver",
            "Transform",
            "UnlitMaterial",
            "MeshInstance",
            "UnlitTexture",
            "Camera",
            "PbrMaterial",
            "Light",
        ]
        if "skeletal-animation" in features:
            expected.extend(["Skeleton", "Skin"])
        expected.extend(["BoundingGeometry", "PickingGeometry"])
        if "mesh-poses" in features:
            expected.append("MeshPose")
        expected.extend(["Hierarchy", "LookAt", "BaseColorTexture", "CustomMaterial"])
        if "particles" in features:
            expected.extend(
                [
                    "ParticleEmitter",
                    "ParticlePlayback",
                    "ParticleSprite",
                    "ParticleMesh",
                ]
            )
        if "surfaces" in features:
            expected.append("Surface")
        if "gui" in features:
            expected.append("GuiRoot")
        if names != expected:
            raise ValueError(f"{configuration}: unexpected compiled registry {names}")
        reports[configuration] = report
        for target in ("native", "wasm"):
            source = directory / f"{target}.contract"
            client = directory / f"{target}.ts"
            duplicate = directory / f"{target}-repeat.ts"
            for path in (client, duplicate):
                run(
                    "cargo",
                    "run",
                    "--quiet",
                    "-p",
                    "ipp-schema-gen",
                    "--locked",
                    "--",
                    str(source),
                    str(path),
                )
            if client.read_bytes() != duplicate.read_bytes():
                raise ValueError(
                    f"{configuration}/{target}: client generation is not reproducible"
                )
            if 'case "attachComponentStateOverlay"' not in client.read_text():
                raise ValueError(
                    f"{configuration}/{target}: baseline overlay codecs missing"
                )
            generated.append(str(client))
        run(
            "node",
            "--input-type=module",
            "--eval",
            "import { assembleClientSupport } from './packages/ipp-client/tools/assemble.mjs'; "
            "assembleClientSupport(process.argv[1]);",
            str(directory),
        )
        has_persistence = (directory / "world-persistence-client.ts").exists()
        if not has_persistence:
            raise ValueError(f"{configuration}: persistence support omission mismatch")
        for target in ("native", "wasm"):
            source = (directory / f"{target}.ts").read_text()
            if ('from "./world-persistence-client.js"' in source) != has_persistence:
                raise ValueError(
                    f"{configuration}/{target}: persistence inheritance mismatch"
                )
    for target in ("native", "wasm"):
        if len({report[target]["hash"] for report in reports.values()}) != len(reports):
            raise ValueError(
                f"{target}: capability selection did not change compatibility identity"
            )
    run(
        "node",
        "node_modules/typescript/bin/tsc",
        "--ignoreConfig",
        "--noEmit",
        "--strict",
        "--target",
        "ES2023",
        "--module",
        "NodeNext",
        "--moduleResolution",
        "NodeNext",
        "--lib",
        "ES2023,DOM",
        *generated,
    )
    print(
        f"Verified {len(reports)} reproducible native/WASM feature contracts and {len(generated)} generated TypeScript clients."
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Contract check failed: {error}", file=sys.stderr)
        sys.exit(1)
