"""Target compilation and generation; Node operations never schedule prerequisites."""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import tempfile
from typing import Iterator

from .artifacts import digest, source_identity, write_json
from .catalog import PRODUCT_SCRIPTS, PROFILES, ProductScript
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


def script_product(entry: ProductScript) -> None:
    with product(ROOT / entry.destination) as directory:
        if entry.runner == "python":
            run([development_python(), entry.script, *entry.arguments, str(directory)])
        elif entry.runner == "typescript":
            run(
                [
                    node(),
                    "node_modules/typescript/bin/tsc",
                    "--project",
                    entry.script,
                    *entry.arguments,
                    "--outDir",
                    str(directory),
                ]
            )
        else:
            run(
                [node(), entry.script, *entry.arguments],
                env={"IPP_BUILD_OUTPUT": str(directory)},
            )


def gallery_gui_assets() -> None:
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
                "tools/assets/gallery_gui_assets.py",
                str(directory),
            ]
        )


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
                    "tools/assets/import-blender-scene.mjs",
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
    """Prepare one `build:<name>` product; script products come from the catalog."""
    if name in PRODUCT_SCRIPTS:
        script_product(PRODUCT_SCRIPTS[name])
    elif name.startswith("browser:"):
        browser(name.removeprefix("browser:"))
    elif name == "native":
        baseline_native()
    elif name in ("gles-host", "gles-host-instrumentation"):
        with product(ROOT / "target" / name) as directory:
            gles_host(directory, name == "gles-host-instrumentation")
    elif name == "gallery-gui-assets":
        gallery_gui_assets()
    elif name in ("gallery-platformer-assets", "gallery-platformer-native-assets"):
        gallery_platformer_assets(native=name == "gallery-platformer-native-assets")
    elif name in ("world-hosts", "scaling-host"):
        world_hosts(name == "scaling-host")
    elif name == "builtin-exporter":
        builtin_exporter()
    elif name == "skinning-fixtures":
        skinning()
    elif name == "blender-addon":
        from .processes import python_tool

        run(python_tool("tools/ipp.py", "_operation", "blender-addon", "package"))
    elif name == "lifecycle-probes":
        target(
            "bundle",
            "tests/runtime/pages/lifecycle-probes.ts",
            "target/browser-build/lifecycle-probes.js",
        )
    else:
        raise ValueError(f"Unknown build operation: {name}")


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
