#!/usr/bin/env python3
"""Execute the native and WASM target exports and verify reproducible matching clients."""

import json
from pathlib import Path
import sys
import subprocess

from pipeline.processes import node, run as execute

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "target/integration-artifacts/contracts"
TARGETS = ("native", "wasm")
COMPONENTS = [
    "Scalar",
    "LinearDriver",
    "ExpressionDriver",
    "Transform",
    "UnlitMaterial",
    "MeshInstance",
    "UnlitTexture",
    "Camera",
    "PbrMaterial",
    "Light",
    "Skeleton",
    "Skin",
    "BoundingGeometry",
    "PickingGeometry",
    "MeshPose",
    "ParentJoint",
    "LookAt",
    "BaseColorTexture",
    "CustomMaterial",
    "ParticleEmitter",
    "ParticlePlayback",
    "ParticleSprite",
    "ParticleMesh",
    "FlatSurface",
    "SurfaceCache",
    "WorldAttachment",
    "CanvasStyle",
    "CanvasText",
    "CanvasGlyphRun",
    "CanvasDrawing",
    "CanvasBitmap",
    "CanvasBox",
    "GuiBehavior",
    "GuiButton",
    "GuiCheckbox",
    "GuiSlider",
    "GuiTextInput",
    "GuiLayout",
    "GuiTheme",
    "GuiSkin",
    "GuiFont",
    "GuiThemeMotion",
    "GuiScrollView",
    "GuiVirtualList",
    "GuiVirtualItem",
    "CanvasBounds",
    "GuiOverlay",
    "GuiGroup",
    "CanvasPaint",
    "GuiColor",
    "CylinderSurface",
    "SphereSurface",
    "BufferDataSourceBinding",
    "StreamingDataSourceBinding",
    "PlotFrame2d",
    "PlotFrame3d",
    "PlotLine2d",
    "PlotBars2d",
    "PlotPie2d",
    "PlotGridBars3d",
    "PlotHeightSurface3d",
    "PlotPoints3d",
    "PlotPie3d",
]


def run(*args, output=None):
    command = [node() if part == "node" else part for part in args]
    execute(command, output=output)


def export(example, output, *features):
    run(
        "cargo",
        "run",
        "--quiet",
        "-p",
        "ipp-protocol",
        "--locked",
        *(["--features", ",".join(features)] if features else []),
        "--example",
        example,
        output=output,
    )


def main():
    """Verify the one contract of each host target and the clients generated from it."""
    directory = ARTIFACTS
    directory.mkdir(parents=True, exist_ok=True)
    native = directory / "native.contract"
    fixture = directory / "native.fixture"
    repeated = directory / "repeated.contract"
    export("export_contract", native)
    export("export_contract", repeated)
    if native.read_bytes() != repeated.read_bytes():
        raise ValueError("native export is not reproducible")
    export("export_fixture", fixture)
    run(
        "cargo",
        "build",
        "-p",
        "ipp-wasm",
        "--locked",
        "--target",
        "wasm32-unknown-unknown",
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
    if names != COMPONENTS:
        raise ValueError(f"unexpected compiled registry {names}")
    generated = []
    for target in TARGETS:
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
            raise ValueError(f"{target}: client generation is not reproducible")
        text = client.read_text()
        if 'case "setFieldIf"' not in text:
            raise ValueError(f"{target}: baseline command codecs missing")
        if 'from "./world-persistence-client.js"' not in text:
            raise ValueError(f"{target}: persistence client inheritance missing")
        generated.append(str(client))
    run(
        "node",
        "--input-type=module",
        "--eval",
        "import { assembleClientSupport } from './packages/ipp-client/tools/assemble.mjs'; "
        "assembleClientSupport(process.argv[1]);",
        str(directory),
    )
    run("node", "tools/ipp-schema-gen/tests/rows-contract.mjs", str(directory))
    export("data_authoring_fixture", directory / "data-authoring.json")
    run(
        "node", "tools/ipp-schema-gen/tests/data-authoring-contract.mjs", str(directory)
    )
    generated.extend(str(directory / f"{target}-rows-types.ts") for target in TARGETS)
    generated.extend(
        str(directory / f"{target}-authoring-types.ts") for target in TARGETS
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
        f"Verified reproducible native/WASM target contracts and {len(generated)} generated TypeScript clients."
    )


def identities():
    """Each host target has one contract identity, whatever its instrumentation."""
    production = (ARTIFACTS / "native.contract").read_bytes()
    for feature in ("instrumentation",):
        output = ARTIFACTS / f"native-{feature}.contract"
        export("export_contract", output, feature)
        if output.read_bytes() != production:
            raise ValueError(f"native: {feature} changed the target contract")
    report = json.loads((ARTIFACTS / "target-report.json").read_text())
    if report["native"]["hash"] == report["wasm"]["hash"]:
        raise ValueError("native and WASM targets have equal contract identity")
    print("Verified one contract identity per host target across instrumentation.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Contract check failed: {error}", file=sys.stderr)
        sys.exit(1)
