"""Opt-in performance products and scenes, separate from regression selections."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import time

from .artifacts import source_identity, write_json
from .builds import cargo, compile_client, product, target, wasm
from .model import ROOT, Task
from .processes import blender, node, run


FEATURES = [
    "builtin-assets",
    "shadows",
    "skeletal-animation",
    "mesh-poses",
    "particles",
]
DIRECTORY = ROOT / "target/performance-build"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_native(instrumented: bool) -> None:
    features = [*FEATURES, *(["profiling"] if instrumented else [])]
    name = "native-instrumented" if instrumented else "native"
    with product(DIRECTORY / name) as directory:
        cargo(
            "build",
            "--release",
            "-p",
            "ipp-server",
            "--bin",
            "ipp-server",
            "--example",
            "profile_scene",
            "--features",
            ",".join(["websocket", "render", *features]),
        )
        suffix = ".exe" if os.name == "nt" else ""
        for source, destination in (
            (f"ipp-server{suffix}", f"ipp-server{suffix}"),
            (f"examples/profile_scene{suffix}", f"profile_scene{suffix}"),
        ):
            shutil.copy2(ROOT / "target/release" / source, directory / destination)
        cargo(
            "run",
            "--release",
            "--quiet",
            "-p",
            "ipp-protocol",
            "--example",
            "export_contract",
            "--features",
            ",".join(["schema-export", *features]),
            output=directory / "contract.bin",
        )
        compile_client(directory, directory / "contract.bin")
        write_json(
            directory / "build-identity.json",
            {
                "source": source_identity(ROOT),
                "profile": "release",
                "capabilities": features,
                "instrumented": instrumented,
                "executable": digest(directory / f"profile_scene{suffix}"),
                "contract": digest(directory / "contract.bin"),
            },
        )


def build_browser() -> None:
    # Instrumentation is explicit and cannot change a normal distribution build.
    with product(DIRECTORY / "browser") as output:
        for name in ("headless", "render-expanded"):
            directory = output / name
            directory.mkdir()
            features = ["profiling"]
            if name == "render-expanded":
                features.extend(["render", "diagnostics", *FEATURES[1:]])
            builtins = name == "render-expanded"
            wasm(features, builtins, "release-small", directory)
            compile_client(directory, directory / "contract.bin")
            request = directory / "request.json"
            write_json(
                request,
                {
                    "configuration": name,
                    "features": features,
                    "builtins": builtins,
                    "directory": str(directory),
                },
            )
            run([node(), "tools/build/verify-browser.mjs", str(request)])
            request.unlink()


def register(tasks: dict[str, Task]) -> None:
    from .catalog import operation

    for name in ("native", "native-instrumented", "browser"):
        tasks[f"build:performance-{name}"] = Task(
            f"build:performance-{name}",
            f"Prepare opt-in {name} performance host",
            operation("benchmark-build", name),
            ("build:client",),
            ("node", "npm", "rust", *(["wasm"] if name == "browser" else [])),
            (f"target/performance-build/{name}",),
        )


def plan(args: argparse.Namespace, tasks: dict[str, Task]) -> list[str]:
    from .catalog import operation

    if args.frames < 1 or args.group < 1:
        raise ValueError("Frames and controller group size must be positive")
    if sum((args.culling_views, args.draw_sweep, args.render_profile)) > 1:
        raise ValueError("Select one native benchmark scenario")
    if args.backend == "browser" and (
        args.culling_views
        or args.draw_sweep
        or args.render_profile
        or args.geometry_index
        or args.skip_moving
        or args.allow_software
    ):
        raise ValueError(
            "Camera stations, draw controls, index selection and software permission require native GLES"
        )
    if args.backend == "native" and (args.compare_culling or args.group != 64):
        raise ValueError(
            "Bounds insertion comparison and controller grouping require the browser backend"
        )
    name = (
        "browser"
        if args.backend == "browser"
        else ("native-instrumented" if args.instrumented else "native")
    )
    build_id = f"build:performance-{name}"
    if args.build_only:
        return [build_id]
    config = {
        key: getattr(args, key)
        for key in (
            "backend",
            "preset",
            "frames",
            "group",
            "instrumented",
            "output",
            "scene_dir",
            "bundle_dir",
            "reuse_scene",
            "reuse_import",
            "allow_software",
            "skip_moving",
            "culling_views",
            "draw_sweep",
            "render_profile",
            "geometry_index",
            "compare_culling",
            "egl_dir",
        )
    }
    config["host"] = str(DIRECTORY / name)
    requirements = ["node", "npm"]
    if not args.reuse_scene:
        requirements.append("blender")
    if args.backend == "browser":
        requirements.append("browser")
    else:
        requirements.append("gles")
    tasks["benchmark:scene"] = Task(
        "benchmark:scene",
        "Profile the opt-in Blender stress scene",
        operation("benchmark", json.dumps(config)),
        () if args.reuse_build else (build_id,),
        tuple(requirements),
        timeout=14400,
    )
    return ["benchmark:scene"]


def scene(config: dict) -> None:
    directory = Path(
        config["scene_dir"] or f"target/stress-benchmark/{config['preset']}"
    ).resolve()
    output = Path(
        config["output"]
        or f"target/stress-benchmark/{config['backend']}-{config['preset']}"
    ).resolve()
    bundle = Path(config["bundle_dir"] or output / "bundle").resolve()
    host = Path(config["host"])
    output.mkdir(parents=True, exist_ok=True)
    if not config["reuse_scene"]:
        directory.mkdir(parents=True, exist_ok=True)
        run(
            [
                blender(),
                "--background",
                "--factory-startup",
                "--python-exit-code",
                "1",
                "--python",
                "tests/blender/stress_scene.py",
                "--",
                "--output",
                str(directory),
                "--grid",
                "100" if config["preset"] == "full" else "8",
                "--particles",
                "2000" if config["preset"] == "full" else "200",
            ],
            timeout=14400,
        )
        run(
            [
                blender(),
                "--background",
                str(directory / "benchmark.blend"),
                "--python-exit-code",
                "1",
                "--python",
                "integrations/blender/export_scene.py",
                "--",
                str(directory / "export"),
            ],
            timeout=14400,
        )
    fixture = json.loads((directory / "fixture.json").read_text())
    if fixture.get("version") != 2:
        raise ValueError(
            "Regenerate the stress scene without --reuse-scene: expected feature fixture version 2"
        )
    if config["backend"] == "browser":
        browser_scene(config, directory, bundle, output)
        return
    rows = []
    for probe in fixture["probes"]:
        x, y, z = probe["position_blender"]
        w, qx, qy, qz = probe["quaternion_wxyz"]
        rows.append(
            "\t".join(
                map(
                    str,
                    (
                        probe["name"],
                        (probe["frame"] - 1) / fixture["fps"],
                        x,
                        z,
                        -y,
                        qx,
                        qz,
                        -qy,
                        w,
                    ),
                )
            )
        )
    (output / "probes.tsv").write_text("\n".join(rows) + "\n")
    pose_rows = []
    for probe in fixture["pose_probes"]:
        for x, y, z in probe["vertices_blender"]:
            pose_rows.append(
                "\t".join(
                    map(
                        str,
                        (
                            probe["name"],
                            (probe["frame"] - 1) / fixture["fps"],
                            probe["weight"],
                            x,
                            z,
                            -y,
                        ),
                    )
                )
            )
    (output / "pose-probes.tsv").write_text("\n".join(pose_rows) + "\n")
    if not config["reuse_import"]:
        importer = output / "import.mjs"
        run([node(), "tools/build/performance.mjs", "native-import", str(importer)])
        run(
            [node(), str(importer), str(directory / "export"), str(bundle), str(host)],
            timeout=3600,
        )
    suffix = ".exe" if os.name == "nt" else ""
    identity = json.loads((host / "build-identity.json").read_text())
    if identity["executable"] != digest(host / f"profile_scene{suffix}"):
        raise ValueError("Native benchmark executable differs from its build identity")
    write_json(
        output / "run-identity.json",
        {
            "started": time.time(),
            "arguments": config,
            "machine": platform.uname()._asdict(),
            "build": identity,
            "fixture": digest(directory / "fixture.json"),
            "world": digest(bundle / "benchmark.ipp"),
        },
    )
    flags = [
        f"--{key.replace('_', '-')}"
        for key in (
            "allow_software",
            "skip_moving",
            "culling_views",
            "draw_sweep",
            "render_profile",
        )
        if config[key]
    ]
    if config["geometry_index"]:
        flags.append(f"--geometry-index={config['geometry_index']}")
    run(
        [
            str(host / f"profile_scene{suffix}"),
            config["egl_dir"] or "/lib64",
            str(bundle),
            str(output),
            str(config["frames"]),
            *flags,
        ],
        timeout=3600,
    )


def browser_scene(config: dict, directory: Path, bundle: Path, output: Path) -> None:
    # The browser harness expects the fixture, bundle and report in one directory.
    output.mkdir(parents=True, exist_ok=True)
    if directory != output:
        shutil.copy2(directory / "fixture.json", output / "fixture.json")
    if bundle != output / "bundle":
        raise ValueError("Browser benchmarks require --bundle-dir OUTPUT/bundle")
    env = {
        "IPP_BROWSER_BUILD_DIR": config["host"],
        "IPP_STRESS_DIR": str(output),
        "IPP_STRESS_FRAMES": str(config["frames"]),
        "IPP_STRESS_GROUP": str(config["group"]),
        "IPP_STRESS_COMPARE_CULLING": "1" if config["compare_culling"] else "0",
    }
    if not config["reuse_import"]:
        run(
            [
                node(),
                "tools/import_blender_scene.mjs",
                str(directory / "export"),
                str(bundle),
                "--namespace",
                "stress",
                "--world",
                "benchmark.ipp",
                "--clips-only",
                "--defer-presentation",
            ],
            env=env,
            timeout=3600,
        )
    target_path = output / "profile.mjs"
    run([node(), "tools/build/performance.mjs", "stress", str(target_path)], env=env)
    run([node(), "--test", str(target_path)], env=env, timeout=3600)
