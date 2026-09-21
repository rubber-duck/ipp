"""Target compilation and generation; Node operations never schedule prerequisites."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import tempfile
from typing import Iterator

from .artifacts import write_json
from .catalog import PROFILES
from .model import ROOT
from .processes import blender, node, run
from .environment import development_python


def cargo(*args: str, output: Path | None = None) -> None:
    run(["cargo", *args, "--locked"], output=output)


def target(action: str, *args: str | Path) -> None:
    run([node(), "tools/build/target.mjs", action, *(str(arg) for arg in args)])


def compile_client(
    directory: Path, contract: Path, *, compile_types: bool = True
) -> None:
    source = directory / "generated.ts"
    run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "ipp-schema-gen",
            "--locked",
            "--",
            str(contract),
            str(source),
        ]
    )
    target("support", directory)
    if compile_types:
        run(
            [
                node(),
                "node_modules/typescript/bin/tsc",
                "--ignoreConfig",
                "--strict",
                "--target",
                "ES2023",
                "--module",
                "NodeNext",
                "--moduleResolution",
                "NodeNext",
                "--lib",
                "ES2023,DOM",
                str(source),
            ]
        )


@contextmanager
def product(destination: Path) -> Iterator[Path]:
    """Build in isolation, then swap a verified product while holding the pipeline lock."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = Path(
        tempfile.mkdtemp(prefix=f".{destination.name}-", dir=destination.parent)
    )
    previous = destination.with_name(f".{destination.name}-previous-{os.getpid()}")
    try:
        yield temporary
        # Reports name the published files, never the transient preparation directory.
        replacements = (
            (str(temporary), str(destination)),
            (temporary.as_posix(), destination.as_posix()),
            (str(temporary.relative_to(ROOT)), str(destination.relative_to(ROOT))),
            (
                temporary.relative_to(ROOT).as_posix(),
                destination.relative_to(ROOT).as_posix(),
            ),
        )

        def published_paths(value: object) -> object:
            if isinstance(value, str):
                for source, target in replacements:
                    value = value.replace(source, target)
            elif isinstance(value, list):
                return [published_paths(item) for item in value]
            elif isinstance(value, dict):
                return {key: published_paths(item) for key, item in value.items()}
            return value

        for report in temporary.rglob("*.json"):
            if report.name.endswith("report.json"):
                write_json(report, published_paths(json.loads(report.read_text())))
        if destination.exists():
            destination.rename(previous)
        try:
            temporary.rename(destination)
        except BaseException:
            if previous.exists():
                previous.rename(destination)
            raise
        if previous.exists():
            shutil.rmtree(previous)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def wasm(features: list[str], builtins: bool, profile: str, directory: Path) -> None:
    profile_flags = [] if profile == "debug" else ["--profile", profile]
    flags = [] if builtins else ["--no-default-features"]
    for export, name in ((True, "export.wasm"), (False, "runtime.wasm")):
        selection = [*features, *(["schema-export"] if export else [])]
        cargo(
            "build",
            "-p",
            "ipp-wasm",
            "--target",
            "wasm32-unknown-unknown",
            *profile_flags,
            *flags,
            *(["--features", ",".join(selection)] if selection else []),
        )
        shutil.copy2(
            ROOT / "target/wasm32-unknown-unknown" / profile / "ipp_wasm.wasm",
            directory / name,
        )
        if export:
            target("export", directory / name, directory / "contract.bin")


def browser(name: str) -> None:
    configuration = PROFILES["browser"][name]
    with product(ROOT / "target/browser-build" / name) as directory:
        wasm(
            configuration["features"],
            configuration["builtins"],
            "release-small",
            directory,
        )
        compile_client(directory, directory / "contract.bin")
        request = directory / "request.json"
        write_json(
            request,
            {"configuration": name, **configuration, "directory": str(directory)},
        )
        run([node(), "tools/build/verify-browser.mjs", str(request)])
        request.unlink()


def native_host(directory: Path, features: list[str], *, release: bool = False) -> None:
    flags = ["--release"] if release else []
    cargo(
        "build",
        "-p",
        "ipp-server",
        "--no-default-features",
        "--features",
        ",".join(["websocket", *features]),
        *flags,
    )
    executable = "ipp-server.exe" if os.name == "nt" else "ipp-server"
    shutil.copy2(
        ROOT / "target" / ("release" if release else "debug") / executable,
        directory / executable,
    )
    cargo(
        "run",
        "--quiet",
        "-p",
        "ipp-protocol",
        "--example",
        "export_contract",
        "--no-default-features",
        "--features",
        ",".join(["schema-export", *(f for f in features if f != "diagnostics")]),
        *flags,
        output=directory / "contract.bin",
    )


def baseline_native() -> None:
    artifacts = ROOT / "target/integration-artifacts"
    with product(artifacts / "native") as native:
        native_host(native, ["diagnostics"])
        with product(artifacts / "client") as client:
            compile_client(client, native / "contract.bin", compile_types=False)
        shutil.copy2(native / "contract.bin", artifacts / "native.contract")


def world_hosts(release: bool = False) -> None:
    with product(
        ROOT / ("target/scaling-host-build" if release else "target/world-host-build")
    ) as output:
        for name in ["native"] if release else ["native", "wasm"]:
            directory = output / name
            directory.mkdir()
            if name == "native":
                native_host(directory, ["builtin-assets"], release=release)
            else:
                wasm(["builtin-assets"], False, "debug", directory)
            compile_client(directory, directory / "contract.bin")
            target("world", directory, name)


def builtin_exporter() -> None:
    cargo(
        "build",
        "-p",
        "ipp-core",
        "--example",
        "export_builtin",
        "--features",
        "builtin-assets,skeletal-animation",
    )
    executable = "export_builtin.exe" if os.name == "nt" else "export_builtin"
    with product(ROOT / "target/builtin-exporter") as directory:
        shutil.copy2(
            ROOT / "target/debug/examples" / executable, directory / executable
        )


def skinning() -> None:
    executable = (
        ROOT
        / "target/builtin-exporter"
        / ("export_builtin.exe" if os.name == "nt" else "export_builtin")
    )
    with product(ROOT / "target/skinning-build") as directory:
        for kind, uri, name in (
            ("mesh", "ipp://mesh/rig-strip", "rig.mesh"),
            ("skeleton", "ipp://skeleton/rig-strip", "rig.skeleton"),
            ("skin", "ipp://skin/rig-strip", "rig.skin"),
            ("pose", "ipp://pose/rig-strip-bent", "bent.pose"),
        ):
            run([str(executable), kind, uri], output=directory / name)


def node_product(script: str, destination: str, *args: str) -> None:
    with product(ROOT / destination) as directory:
        run([node(), script, *args], env={"IPP_BUILD_OUTPUT": str(directory)})


def gallery_platformer_assets() -> None:
    authoring = ROOT / "examples/world-gallery/worlds/platformer/authoring"
    with product(ROOT / "target/gallery-platformer-assets") as directory:
        with tempfile.TemporaryDirectory(
            prefix="platformer-export-", dir=ROOT / "target"
        ) as exported:
            run(
                [
                    blender(),
                    "--background",
                    "--factory-startup",
                    "--python-exit-code",
                    "17",
                    str(authoring / "platformer.blend"),
                    "--python",
                    "integrations/blender/export_scene.py",
                    "--",
                    exported,
                ]
            )
            run(
                [
                    node(),
                    "tools/import_blender_scene.mjs",
                    exported,
                    str(directory),
                    "--namespace",
                    "platformer",
                    "--world",
                    "platformer.ipp",
                    "--clips-only",
                ]
            )
        run(
            [
                development_python(),
                str(authoring / "route.py"),
                str(directory / "route.json"),
            ]
        )


def build(name: str) -> None:
    if name.startswith("browser:"):
        browser(name.removeprefix("browser:"))
    elif name == "native":
        baseline_native()
    elif name == "surface-host":
        with product(ROOT / "target/surface-host") as directory:
            native_host(directory, ["surfaces"])
            compile_client(directory, directory / "contract.bin")
    elif name == "gui-host":
        with product(ROOT / "target/gui-host") as directory:
            native_host(directory, ["surfaces", "gui"])
            compile_client(directory, directory / "contract.bin")
    elif name == "font-assets":
        with product(ROOT / "target/font-assets") as directory:
            run([development_python(), "tools/build_font_assets.py", str(directory)])
    elif name == "surface-assets":
        with product(ROOT / "target/surface-assets") as directory:
            run([development_python(), "tools/build_surface_assets.py", str(directory)])
    elif name == "gallery-gui-assets":
        with product(ROOT / "target/gallery-gui-assets") as directory:
            authoring = ROOT / "examples/world-gallery/worlds/gui/authoring"
            run(
                [
                    blender(),
                    "--background",
                    "--factory-startup",
                    "--python-exit-code",
                    "17",
                    str(authoring / "projector.blend"),
                    "--python",
                    str(authoring / "build_projector.py"),
                    "--",
                    "--export-only",
                    "--output-directory",
                    str(directory / "projector"),
                ]
            )
            run(
                [
                    development_python(),
                    "tools/build_gallery_gui_assets.py",
                    str(directory),
                ]
            )
    elif name == "gallery-platformer-assets":
        gallery_platformer_assets()
    elif name in ("world-hosts", "scaling-host"):
        world_hosts(name == "scaling-host")
    elif name == "builtin-exporter":
        builtin_exporter()
    elif name == "skinning-fixtures":
        skinning()
    elif name == "client":
        with product(ROOT / "packages/ipp-client/dist") as directory:
            run(
                [
                    node(),
                    "node_modules/typescript/bin/tsc",
                    "--project",
                    "packages/ipp-client/tsconfig.build.json",
                    "--outDir",
                    str(directory),
                ]
            )
    elif name == "typescript":
        with product(ROOT / "dist") as directory:
            run(
                [
                    node(),
                    "node_modules/typescript/bin/tsc",
                    "--project",
                    "tsconfig.json",
                    "--outDir",
                    str(directory),
                ]
            )
    elif name == "react":
        node_product("packages/ipp-react/tools/build.mjs", "packages/ipp-react/dist")
    elif name == "blender-addon":
        from .processes import python_tool

        run(python_tool("tools/blender.py", "package"))
    elif name == "lifecycle-probes":
        target(
            "bundle",
            "tests/browser/lifecycle-probes.ts",
            "target/browser-build/lifecycle-probes.js",
        )
    elif name == "blender-fixtures":
        target(
            "bundle",
            "tests/render/blender-fixture.tsx",
            "target/blender-test/blender-fixture.js",
        )
    elif name == "gallery-site":
        node_product("tools/build_gallery.mjs", "target/gallery-site", "site")
    elif name in ("gallery", "gallery-fixtures"):
        node_product(
            "tools/build_gallery.mjs",
            "target/gallery-build" if name == "gallery" else "target/gallery-fixtures",
            "application" if name == "gallery" else "fixtures",
        )
    else:
        scripts = {
            "react-fixtures": "react",
            "canvas-fixtures": "canvas",
            "textures": "textures",
            "shapes": "shapes",
            "surface-fixtures": "surfaces",
            "mesh-pose-fixtures": "mesh_poses",
            "blender-viewer": "blender_viewer",
        }
        if name not in scripts:
            raise ValueError(f"Unknown build operation: {name}")
        destinations = {
            "react-fixtures": "react-build",
            "canvas-fixtures": "canvas-build",
            "textures": "texture-build",
            "shapes": "shapes-build",
            "surface-fixtures": "surface-build",
            "mesh-pose-fixtures": "mesh-pose-build",
            "blender-viewer": "blender-viewer",
        }
        node_product(f"tools/build_{scripts[name]}.mjs", f"target/{destinations[name]}")


def verify_browser_identities() -> None:
    hashes: dict[str, str] = {}
    for name, profile in PROFILES["browser"].items():
        report = json.loads(
            (ROOT / "target/browser-build" / name / "build-report.json").read_text()
        )
        key = json.dumps(
            [
                profile["builtins"],
                *[
                    f in profile["features"]
                    for f in (
                        "skeletal-animation",
                        "mesh-poses",
                        "shadows",
                        "particles",
                        "surfaces",
                        "gui",
                    )
                ],
            ]
        )
        value = report["schemaHash"]
        if key in hashes and hashes[key] != value:
            raise ValueError(
                f"GPU/diagnostics selection changed authored schema: {name}"
            )
        hashes[key] = value
    if len(set(hashes.values())) != len(hashes):
        raise ValueError("Distinct scene capabilities have equal schema identities")
