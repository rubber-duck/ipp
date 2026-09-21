#!/usr/bin/env python3
"""Check crate boundaries and representative feature graphs using actual Cargo resolution."""

import json
from pathlib import Path
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
CORE = "ipp-core"
RENDER = "ipp-render-gl"
HOST_SESSION = "ipp-host-session"
MACROS = "ipp-schema-derive"
HOSTS = ("ipp-server", "ipp-wasm")
BUILTIN_DEFAULTS = {CORE, "ipp-protocol", HOST_SESSION, *HOSTS}
EDGES = {
    CORE: {MACROS},
    "ipp-protocol": {CORE},
    HOST_SESSION: {CORE, "ipp-protocol"},
    RENDER: {CORE},
    **{host: {CORE, "ipp-protocol", HOST_SESSION, RENDER} for host in HOSTS},
    MACROS: set(),
    "ipp-schema-gen": set(),
}


def run(*command):
    result = subprocess.run(
        command, cwd=ROOT, text=True, capture_output=True, timeout=120
    )
    if result.returncode:
        raise ValueError(f"{' '.join(command)} failed:\n{result.stderr}")
    return result.stdout


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_manifests():
    metadata = json.loads(
        run("cargo", "metadata", "--format-version", "1", "--no-deps", "--locked")
    )
    packages = {p["name"]: p for p in metadata["packages"]}
    require(
        packages.keys() == EDGES.keys(),
        "Update the reviewed crate policy when changing workspace membership",
    )
    allowed = {
        "default",
        "builtin-assets",
        "skeletal-animation",
        "mesh-poses",
        "particles",
        "surfaces",
        "gui",
        "shadows",
        "render",
        "websocket",
        "zip-data-source",
        "diagnostics",
        "schema-export",
        # Opt-in profiling instrumentation; never included in a runtime default.
        "profiling",
    }
    declared = {
        feature for package in packages.values() for feature in package["features"]
    }
    require(
        declared == allowed,
        f"Unexpected workspace feature surface: {declared ^ allowed}",
    )
    for name, package in packages.items():
        require(
            package["features"].get("default")
            == (["builtin-assets"] if name in BUILTIN_DEFAULTS else []),
            f"{name}: default capabilities differ from the architecture",
        )
        internal = {d["name"] for d in package["dependencies"] if d["name"] in EDGES}
        require(
            internal == EDGES[name],
            f"{name}: internal dependencies differ from the architecture",
        )
        manifest = tomllib.loads(Path(package["manifest_path"]).read_text())
        tables = [manifest, *manifest.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for dependency, spec in table.get(kind, {}).items():
                    require(
                        isinstance(spec, dict) and spec.get("workspace") is True,
                        f"{name}/{dependency}: declare dependency versions in the workspace",
                    )
        for dependency in package["dependencies"]:
            if dependency["kind"] != "dev":
                require(
                    not dependency["uses_default_features"],
                    f"{name}/{dependency['name']}: disable defaults and select needed features",
                )
    require(
        any("proc-macro" in t["kind"] for t in packages[MACROS]["targets"]),
        f"{MACROS} must remain a host-compiled proc-macro crate",
    )
    for host in HOSTS:
        require(
            any(
                d["name"] == RENDER and d["optional"]
                for d in packages[host]["dependencies"]
            ),
            f"{host}: rendering must be optional",
        )


def graph(package, target, features, default_features=False):
    command = [
        "cargo",
        "tree",
        "--locked",
        "-p",
        package,
        "--target",
        target,
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--format",
        "{p}|{f}",
        "--no-dedupe",
    ]
    if not default_features:
        command += ["--no-default-features"]
    if features:
        command += ["--features", ",".join(features)]
    resolved = {}
    for line in run(*command).splitlines():
        identity, flags = line.rsplit("|", 1)
        resolved.setdefault(identity.split()[0], set()).update(
            filter(None, flags.split(","))
        )
    return resolved


def check_graph(
    package,
    target,
    features,
    core_features,
    renderer_features=None,
    *,
    default_features=False,
):
    resolved = graph(package, target, features, default_features)
    selection = ["default", *features] if default_features else features
    label = f"{package} [{','.join(selection) or 'minimal'}] on {target}"
    require(
        resolved.get(CORE) == set(core_features),
        f"{label}: unexpected core features: {resolved.get(CORE)}",
    )
    require(
        (RENDER in resolved) == (renderer_features is not None),
        f"{label}: unexpected renderer dependency",
    )
    if renderer_features is not None:
        require(
            resolved[RENDER] == set(renderer_features),
            f"{label}: incorrect renderer feature forwarding",
        )
    expected = {CORE, MACROS, package}
    if package in (*HOSTS, HOST_SESSION):
        expected.add("ipp-protocol")
    if package in HOSTS:
        expected.add(HOST_SESSION)
        require(
            resolved[HOST_SESSION] == set(core_features),
            f"{label}: shared session features differ from the host: "
            f"{resolved[HOST_SESSION]}",
        )
    if renderer_features is not None:
        expected.add(RENDER)
    require(
        set(resolved) & EDGES.keys() == expected,
        f"{label}: unexpected internal dependency",
    )
    websocket = package == "ipp-server" and "websocket" in features
    require(
        ("tungstenite" in resolved) == websocket,
        f"{label}: WebSocket dependencies must be isolated to the selected native transport",
    )
    require(
        ("miniz_oxide" in resolved) == ("zip-data-source" in features),
        f"{label}: ZIP decoder dependencies must remain optional",
    )
    if websocket:
        require(
            resolved["tungstenite"]
            == {"data-encoding", "handshake", "http", "httparse", "sha1"},
            f"{label}: unexpected WebSocket dependency features",
        )


def main():
    check_manifests()
    native = next(
        line.removeprefix("host: ")
        for line in run("rustc", "-vV").splitlines()
        if line.startswith("host: ")
    )
    count = 0
    scene = [
        "builtin-assets",
        "skeletal-animation",
        "mesh-poses",
        "shadows",
        "particles",
        "surfaces",
    ]
    for target in (native, "wasm32-unknown-unknown"):
        for package in (CORE, "ipp-protocol", HOST_SESSION):
            check_graph(package, target, [], [])
            check_graph(
                package,
                target,
                [],
                ["builtin-assets"] + (["default"] if package == CORE else []),
                default_features=True,
            )
            count += 2
            for feature in scene:
                check_graph(package, target, [feature], [feature])
                count += 1
        check_graph(CORE, target, ["zip-data-source"], ["zip-data-source"])
        check_graph("ipp-protocol", target, ["schema-export"], [])
        check_graph(RENDER, target, [], [], [])
        check_graph(
            RENDER,
            target,
            ["skeletal-animation", "mesh-poses", "shadows", "particles", "surfaces"],
            ["skeletal-animation", "mesh-poses", "shadows", "particles", "surfaces"],
            ["skeletal-animation", "mesh-poses", "shadows", "particles", "surfaces"],
        )
        count += 4
    for host, target in zip(HOSTS, (native, "wasm32-unknown-unknown")):
        for features, core, renderer in (
            ([], [], None),
            (["render"], [], []),
            (["diagnostics"], ["diagnostics"], None),
            (["builtin-assets"], ["builtin-assets"], None),
            (["skeletal-animation"], ["skeletal-animation"], None),
            (["mesh-poses"], ["mesh-poses"], None),
            (["surfaces"], ["surfaces"], None),
            (["gui"], ["gui", "surfaces"], None),
            (["shadows"], ["shadows"], ["shadows"]),
            (
                ["render", "skeletal-animation"],
                ["skeletal-animation"],
                ["skeletal-animation"],
            ),
            (["render", "mesh-poses"], ["mesh-poses"], ["mesh-poses"]),
            (["render", "surfaces"], ["surfaces"], ["surfaces"]),
            (
                ["render", "gui"],
                ["gui", "surfaces"],
                ["gui", "surfaces"],
            ),
            (
                ["render", "diagnostics", *scene],
                ["diagnostics", *scene],
                [
                    "skeletal-animation",
                    "mesh-poses",
                    "shadows",
                    "particles",
                    "surfaces",
                ],
            ),
        ):
            check_graph(host, target, features, core, renderer)
            count += 1
        check_graph(host, target, [], ["builtin-assets"], default_features=True)
        check_graph(
            host, target, ["render"], ["builtin-assets"], [], default_features=True
        )
        count += 2
    check_graph("ipp-server", native, ["websocket"], [])
    check_graph("ipp-wasm", "wasm32-unknown-unknown", ["schema-export"], [])
    count += 2
    print(
        f"Checked eight crate boundaries and {count} resolved native/WASM feature graphs."
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Workspace check failed: {error}", file=sys.stderr)
        sys.exit(1)
