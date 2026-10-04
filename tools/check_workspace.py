#!/usr/bin/env python3
"""Check crate boundaries and the build-axis feature graphs using actual Cargo resolution."""

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
INSTRUMENTATION = "instrumentation"
# Logging and statistics are compiled into every build; `instrumentation` adds the
# test-only controls and profiling hooks. Builds vary only by instrumentation,
# rendering backend and host target. A World excludes a capability through its
# System selection, never through a feature.
FEATURES = {
    CORE: {"default", INSTRUMENTATION, "checked-invariants"},
    "ipp-protocol": {"default", INSTRUMENTATION},
    HOST_SESSION: {"default", INSTRUMENTATION},
    RENDER: {"default", INSTRUMENTATION},
    "ipp-server": {"default", INSTRUMENTATION},
    # The WASM host links its one renderer only with `render`.
    "ipp-wasm": {"default", INSTRUMENTATION, "render"},
    MACROS: {"default"},
    "ipp-schema-gen": {"default"},
}
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
    for name, package in packages.items():
        declared = set(package["features"])
        require(
            declared == FEATURES[name],
            f"{name}: unexpected features {sorted(declared ^ FEATURES[name])}; "
            "capabilities are always compiled and selected per World",
        )
        require(
            package["features"].get("default") == [],
            f"{name}: default features must be empty",
        )
        # A package's dev-dependency on itself only enables test features.
        internal = {
            d["name"]
            for d in package["dependencies"]
            if d["name"] in EDGES and not (d["name"] == name and d["kind"] == "dev")
        }
        require(
            all(
                d["name"] == CORE == name and d["kind"] == "dev"
                for d in package["dependencies"]
                if "checked-invariants" in d["features"]
            ),
            f"{name}: only ipp-core's own tests may enable checked-invariants",
        )
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
    require(
        any(
            d["name"] == RENDER and d["optional"]
            for d in packages["ipp-wasm"]["dependencies"]
        ),
        "ipp-wasm: rendering must be optional",
    )
    require(
        all(
            d["kind"] == "dev"
            for d in packages["ipp-server"]["dependencies"]
            if d["name"] == RENDER
        ),
        "ipp-server: the renderer serves only its testing examples",
    )


def graph(package, target, features):
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
        "--no-default-features",
    ]
    if features:
        command += ["--features", ",".join(features)]
    resolved = {}
    for line in run(*command).splitlines():
        identity, flags = line.rsplit("|", 1)
        resolved.setdefault(identity.split()[0], set()).update(
            filter(None, flags.split(","))
        )
    return resolved


def check_graph(package, target, features, core_features, renderer_features=None):
    resolved = graph(package, target, features)
    label = f"{package} [{','.join(features) or 'production'}] on {target}"
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
    require(
        "unicode-segmentation" in resolved,
        f"{label}: core text segmentation must be linked into every build",
    )
    scheduled = package in (*HOSTS, HOST_SESSION)
    require(
        ("async-task" in resolved) == scheduled,
        f"{label}: portable task scheduling belongs to Host packages only",
    )
    native_scheduled = scheduled and target != "wasm32-unknown-unknown"
    for dependency in ("async-executor", "async-io", "blocking"):
        require(
            (dependency in resolved) == native_scheduled,
            f"{label}: {dependency} must remain outside core and browser WASM",
        )
    websocket = package == "ipp-server"
    require(
        ("tungstenite" in resolved) == websocket,
        f"{label}: WebSocket dependencies belong only to the native server",
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
    selections = ([], [INSTRUMENTATION])
    for target in (native, "wasm32-unknown-unknown"):
        for package in (CORE, "ipp-protocol", HOST_SESSION):
            for features in selections:
                check_graph(package, target, features, features)
                count += 1
        for features in selections:
            check_graph(RENDER, target, features, features, features)
            count += 1
    for features in selections:
        # The native server has no render feature; its renderer is a
        # dev-dependency of its testing examples.
        check_graph("ipp-server", native, features, features)
        check_graph("ipp-wasm", "wasm32-unknown-unknown", features, features)
        check_graph(
            "ipp-wasm",
            "wasm32-unknown-unknown",
            ["render", *features],
            features,
            features,
        )
        count += 3
    print(
        f"Checked eight crate boundaries and {count} resolved native/WASM feature graphs."
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Workspace check failed: {error}", file=sys.stderr)
        sys.exit(1)
