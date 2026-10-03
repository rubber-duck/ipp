"""Target compilation and generation; Node operations never schedule prerequisites."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import tempfile
from typing import Iterator

from .artifacts import digest, source_identity, write_json
from .catalog import PROFILES
from .model import ROOT
from .processes import blender, node, run
from .environment import development_python


def cargo(*args: str, output: Path | None = None) -> None:
    run(["cargo", *args, "--locked"], output=output)


def target(action: str, *args: str | Path) -> None:
    run([node(), "tools/build/target.mjs", action, *(str(arg) for arg in args)])


def compile_client(directory: Path, contract: Path) -> None:
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


def wasm(features: list[str], profile: str, directory: Path) -> None:
    """Compile the shipped runtime once and read its contract from that module."""
    cargo(
        "build",
        "-p",
        "ipp-wasm",
        "--target",
        "wasm32-unknown-unknown",
        "--profile",
        profile,
        *(["--features", ",".join(features)] if features else []),
    )
    shutil.copy2(
        ROOT / "target/wasm32-unknown-unknown" / profile / "ipp_wasm.wasm",
        directory / "runtime.wasm",
    )
    target("export", directory / "runtime.wasm", directory / "contract.bin")


def browser(name: str) -> None:
    configuration = PROFILES["browser"][name]
    with product(ROOT / "target/browser-build" / name) as directory:
        wasm(configuration["features"], "release-small", directory)
        compile_client(directory, directory / "contract.bin")
        request = directory / "request.json"
        write_json(
            request,
            {"configuration": name, **configuration, "directory": str(directory)},
        )
        run([node(), "tools/build/verify-browser.mjs", str(request)])
        request.unlink()
        write_json(
            directory / "build-identity.json",
            {
                "source": source_identity(ROOT),
                "profile": "release-small",
                "features": configuration["features"],
                "instrumented": "instrumentation" in configuration["features"],
                "runtime": digest(directory / "runtime.wasm"),
                "contract": digest(directory / "contract.bin"),
            },
        )


def native_contract(output: Path, *, release: bool = False) -> None:
    """Export the native target contract; instrumentation never changes it."""
    cargo(
        "run",
        "--quiet",
        "-p",
        "ipp-protocol",
        "--example",
        "export_contract",
        *(["--release"] if release else []),
        output=output,
    )


def native_host(directory: Path, features: list[str], *, release: bool = False) -> None:
    flags = ["--release"] if release else []
    cargo(
        "build",
        "-p",
        "ipp-server",
        *(["--features", ",".join(features)] if features else []),
        *flags,
    )
    executable = "ipp-server.exe" if os.name == "nt" else "ipp-server"
    shutil.copy2(
        ROOT / "target" / ("release" if release else "debug") / executable,
        directory / executable,
    )
    native_contract(directory / "contract.bin", release=release)


def gles_host(directory: Path, instrumentation: bool, *, release: bool = False) -> None:
    """The presenting GLES testing host of ipp-server, its contract and client.

    The instrumentation build honours the testing controls; the normal one is the
    build timing runs measure.
    """
    cargo(
        "build",
        "-p",
        "ipp-server",
        "--example",
        "gles_host",
        *(["--release"] if release else []),
        *(["--features", "instrumentation"] if instrumentation else []),
    )
    suffix = ".exe" if os.name == "nt" else ""
    shutil.copy2(
        ROOT
        / "target"
        / ("release" if release else "debug")
        / f"examples/gles_host{suffix}",
        directory / f"gles_host{suffix}",
    )
    native_contract(directory / "contract.bin", release=release)
    compile_client(directory, directory / "contract.bin")


def baseline_native() -> None:
    artifacts = ROOT / "target/integration-artifacts"
    with product(artifacts / "native") as native:
        native_host(native, [])
        with product(artifacts / "client") as client:
            compile_client(client, native / "contract.bin")
        shutil.copy2(native / "contract.bin", artifacts / "native.contract")


def world_hosts(release: bool = False) -> None:
    destination = "target/scaling-host-build" if release else "target/world-host-build"
    features: list[str] = []
    with product(ROOT / destination) as output:
        for name in ["native"] if release else ["native", "wasm"]:
            directory = output / name
            directory.mkdir()
            if name == "native":
                native_host(directory, features, release=release)
            else:
                # Tests start this runtime in many workers: keep debug
                # assertions and overflow checks, drop the unused debuginfo.
                wasm(features, "wasm-dev", directory)
            compile_client(directory, directory / "contract.bin")
            target("world", directory, name)


def builtin_exporter() -> None:
    cargo(
        "build",
        "-p",
        "ipp-core",
        "--example",
        "export_builtin",
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


def gallery_platformer_assets(*, native: bool = False) -> None:
    authoring = ROOT / "examples/world-gallery/worlds/platformer/authoring"
    name = "gallery-platformer-native-assets" if native else "gallery-platformer-assets"
    with product(ROOT / "target" / name) as directory:
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
                    *(["--native"] if native else []),
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
    elif name == "dataset-fixtures":
        node_product("tools/build_datasets.mjs", "target/datasets")
    elif name == "plots-fixtures":
        node_product("tools/build_plots.mjs", "target/plots")
    elif name == "headless-client":
        node_product("tools/build_headless_client.mjs", "target/headless-client")
    elif name in ("gles-host", "gles-host-instrumentation"):
        with product(ROOT / "target" / name) as directory:
            gles_host(directory, name == "gles-host-instrumentation")
    elif name == "font-assets":
        with product(ROOT / "target/font-assets") as directory:
            run([development_python(), "tools/build_font_assets.py", str(directory)])
    elif name == "surface-assets":
        with product(ROOT / "target/surface-assets") as directory:
            run([development_python(), "tools/build_surface_assets.py", str(directory)])
    elif name == "gallery-assets":
        with product(ROOT / "target/gallery-assets") as directory:
            run(
                [node(), "tools/build_gallery.mjs", "assets"],
                env={"IPP_BUILD_OUTPUT": str(directory)},
            )
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
    elif name in ("gallery-platformer-assets", "gallery-platformer-native-assets"):
        gallery_platformer_assets(native=name == "gallery-platformer-native-assets")
    elif name in ("world-hosts", "scaling-host"):
        world_hosts(name == "scaling-host")
    elif name == "builtin-exporter":
        builtin_exporter()
    elif name == "skinning-fixtures":
        skinning()
    elif name == "render-fixtures":
        node_product("tools/build_render_fixtures.mjs", "target/render-fixtures")
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
    elif name == "gui-motion-fixtures":
        node_product("tests/integration/gui-motion/build.mjs", "target/gui-motion")
    elif name == "gui-composites-fixtures":
        node_product(
            "tests/integration/gui-composites/build.mjs", "target/gui-composites"
        )
    elif name == "gui-default-skin-fixtures":
        node_product(
            "tests/integration/gui-default-skin/build.mjs", "target/gui-default-skin"
        )
    elif name == "transport-fixtures":
        run(
            [
                node(),
                "node_modules/typescript/bin/tsc",
                "--project",
                "tests/integration/tsconfig.transports.json",
            ]
        )
        node_product("tools/build_transports.mjs", "target/multiplex-tests")
    elif name == "composed-query-fixtures":
        node_product("tools/build_composed_queries.mjs", "target/composed-queries")
    elif name in (
        "typescript",
        "asset-rejection-tests",
        "asset-rejection-worker-tests",
    ):
        configuration, destination = {
            "typescript": ("tsconfig.json", "dist"),
            "asset-rejection-tests": (
                "tests/integration/tsconfig.asset-rejection.json",
                "target/asset-rejection-tests",
            ),
            "asset-rejection-worker-tests": (
                "tests/browser/tsconfig.asset-rejection.json",
                "target/asset-rejection-worker-tests",
            ),
        }[name]
        with product(ROOT / destination) as directory:
            run(
                [
                    node(),
                    "node_modules/typescript/bin/tsc",
                    "--project",
                    configuration,
                    "--outDir",
                    str(directory),
                ]
            )
    elif name == "react":
        node_product("packages/ipp-react/tools/build.mjs", "packages/ipp-react/dist")
    elif name == "react-attached":
        node_product("tests/react/build-attached-world.mjs", "target/react-attached")
    elif name == "react-gui-authoring":
        node_product(
            "tests/react/build-gui-authoring.mjs", "target/react-gui-authoring"
        )
    elif name == "gui-stress-fixtures":
        node_product("tools/build_gui_stress.mjs", "target/gui-stress")
    elif name == "worker-profiling-fixture":
        node_product("tools/build/performance.mjs", "target/worker-profiling", "robot")
    elif name == "shared-host":
        node_product("tools/shared-host/build.mjs", "target/shared-host")
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
        node_product("tools/build_blender_fixtures.mjs", "target/blender-test")
    elif name == "blender-viewer-instrumentation":
        node_product(
            "tools/build_blender_viewer.mjs",
            "target/blender-viewer-instrumentation",
            "render-instrumentation",
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
            "surface-gui-fixtures": "surface_gui",
            "mesh-pose-fixtures": "mesh_poses",
            "blender-viewer": "blender_viewer",
            "blender-headless-fixtures": "blender_headless",
        }
        if name not in scripts:
            raise ValueError(f"Unknown build operation: {name}")
        destinations = {
            "react-fixtures": "react-build",
            "canvas-fixtures": "canvas-build",
            "textures": "texture-build",
            "shapes": "shapes-build",
            "surface-fixtures": "surface-build",
            "surface-gui-fixtures": "surface-gui-build",
            "mesh-pose-fixtures": "mesh-pose-build",
            "blender-viewer": "blender-viewer",
            "blender-headless-fixtures": "blender-headless",
        }
        node_product(f"tools/build_{scripts[name]}.mjs", f"target/{destinations[name]}")


def verify_browser_identities() -> None:
    """Instrumentation and the renderer never change the WASM target contract."""
    hashes = {
        name: json.loads(
            (ROOT / "target/browser-build" / name / "build-report.json").read_text()
        )["schemaHash"]
        for name in PROFILES["browser"]
    }
    contract = json.loads(
        (ROOT / "target/integration-artifacts/contracts/target-report.json").read_text()
    )["wasm"]["hash"]
    for name, value in hashes.items():
        if value != contract:
            raise ValueError(
                f"Browser distribution {name} changed the WASM target contract"
            )
