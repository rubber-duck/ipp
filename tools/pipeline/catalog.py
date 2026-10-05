"""The single build, suite and check catalog used by the CLI and CI."""

from dataclasses import dataclass
import json
from pathlib import Path
import sys

from .model import ROOT, Task, select
from .processes import blender, node
from .environment import development_python


DATA = Path(__file__).parent
PROFILES = json.loads((DATA / "profiles.json").read_text())
SUITES = json.loads((DATA / "suites.json").read_text())
TEST_INPUTS = json.loads((DATA / "test-inputs.json").read_text())
GLES_CHECKS = json.loads((DATA / "gles.json").read_text())


@dataclass(frozen=True)
class RegressionGroup:
    description: str
    suites: tuple[str, ...] = ()
    steps: tuple[str, ...] = ()


# Groups reference the maintained suites/checks; commands and prerequisites stay
# in their owning registries. Keep costly additions out of the explicit core set.
REGRESSION_GROUPS = {
    "runtime": RegressionGroup(
        "Worlds, lifecycle, persistence, animation, hierarchy and React through real transports",
        (
            "filesystem",
            "worlds",
            "lifecycle",
            "snapshots",
            "animation",
            "hierarchy",
            "command-streaming",
            "multiplex",
            "react",
            "react-attached",
            "headless-client",
            "datasets",
        ),
        (
            "test:presentation:host-wire",
            "test:asset-exports:cpu-native",
            "test:async-io:native-http",
        ),
    ),
    "browser": RegressionGroup(
        "Chromium worker/WASM transport and lifecycle",
        ("browser",),
        (
            "test:presentation:webgl",
            "test:presentation:webgl-production",
            "test:presentation:diagnostics",
            "test:gui-local:worker",
            "test:multiplex:worker-startup",
            "test:multiplex:task-scheduler",
            "test:async-io:generated-buffers",
            "test:multiplex:bulk-worker",
            "test:asset-exports:gpu-browser",
            "test:react-gui-authoring:worker",
        ),
    ),
    "rendering": RegressionGroup(
        "WebGL frames, cameras, geometry, materials, lighting, deformation and resource recovery",
        (
            "render",
            "platformer-profile",
            "cameras",
            "geometry",
            "custom-materials",
            "lighting",
            "particles",
            "mesh-poses",
            "skinning",
            "textures",
            "shapes",
            "render-residency",
            "render-publications",
            "composed-queries",
        ),
        (
            "test:host-profiling:browser",
            "test:gpu-profiling:browser",
            "test:canvas:lifecycle",
            "test:canvas:dom",
            "test:canvas:controller-webgl",
            "test:canvas:patch-oracle",
            "test:canvas:patch-render",
            "test:plots:core",
            "test:plots:authoring",
            "test:plots:browser",
            "test:plots:dense-browser",
            "test:plots:views-browser",
            "test:plots:3d-browser",
            "test:plots:axis-browser",
            "test:react-gui-authoring:webgl",
            "test:react-gui-authoring:projected-advanced-webgl",
            "test:react-gui-authoring:projected-transitions-webgl",
            "test:react-gui-authoring:projected-recovery-webgl",
            "test:presentation:webgl",
            "test:presentation:webgl-production",
            "test:presentation:diagnostics",
        ),
    ),
    "gui": RegressionGroup(
        "Surface/GUI state, input, retained rendering and Surface cache correctness",
        (
            "surfaces",
            "gui",
            "gui-composites",
            "gui-local",
            "gui-motion",
            "gui-default-skin",
            "output-inclusion",
            "react-gui-authoring",
            "retained-gui",
            "surface-cache",
            "gui-stress",
        ),
    ),
    "gallery": RegressionGroup(
        "Published gallery and interactive GUI, camera, particle and platformer demos",
        (
            "gallery-site",
            "gallery-gui",
            "gallery-gui-camera",
            "gallery-particles",
            "gallery-charts",
            "gallery-chart-input",
            "gallery-platformer",
            "native-gallery",
        ),
        steps=("test:gallery-trace:exporter", "test:gallery-trace:browser"),
    ),
    "blender": RegressionGroup(
        "Blender packaging, exports, streaming and browser presentation",
        ("blender", "blender-headless", "blender-disk-headless", "particles-blender"),
    ),
    "gles": RegressionGroup(
        "All native GLES frame scenarios; requires configured EGL/GLES libraries",
        steps=(
            "test:asset-exports:gpu-native",
            *tuple(record["id"] for record in GLES_CHECKS),
            "test:canvas:controller-gles",
            "test:host-profiling:native-gles",
            "test:gallery-trace:native",
            "test:gpu-profiling:native",
            "test:presentation:native-gles",
            "test:react-gui-authoring:gles",
            "test:react-gui-authoring:projected-advanced-gles",
            "test:react-gui-authoring:projected-transitions-gles",
            "test:react-gui-authoring:projected-recovery-gles",
            "test:plots:native",
            "test:plots:dense-native",
            "test:plots:views-native",
            "test:plots:3d-native",
            "test:plots:axis-native",
        ),
    ),
    "matrix": RegressionGroup(
        "All-features Rust and WASM builds, target contracts and browser distribution identities",
        ("contracts",),
        (
            "test:rust:all-features",
            "check:clippy-all-features",
            "check:wasm-default",
            "check:wasm-all-features",
            "check:browser-identities",
            "check:distribution-sizes",
        ),
    ),
    "scaling": RegressionGroup(
        "Release-mode 1k/4k/16k mutation timings and operation counts", ("scaling",)
    ),
}


def operation(*args: str) -> tuple[str, ...]:
    return (sys.executable, "tools/ipp.py", "_operation", *args)


def catalog(egl_directory: str | None = None) -> dict[str, Task]:
    tasks: dict[str, Task] = {}

    def add(task: Task) -> None:
        if task.id in tasks:
            raise ValueError(f"Duplicate task: {task.id}")
        tasks[task.id] = task

    def build(
        name: str,
        dependencies: tuple[str, ...],
        outputs: tuple[str, ...],
        requirements: tuple[str, ...] = ("node", "npm", "rust"),
    ) -> None:
        # The scheduler runs "rust" steps one at a time; builds without Cargo omit it.
        add(
            Task(
                f"build:{name}",
                f"Prepare {name}",
                operation("build", name),
                tuple(f"build:{d}" for d in dependencies),
                requirements,
                outputs,
            )
        )

    build("client", (), ("packages/ipp-client/dist",), ("node", "npm"))
    add(
        Task(
            "build:presentation-wire-host",
            "Compile the pinned Rust Host cancellation conformance driver",
            (
                node(),
                "crates/ipp-protocol/tests/presentation-cancellation.mjs",
                "--build",
            ),
            requirements=("node", "npm", "rust"),
            outputs=("target/presentation-wire-host",),
        )
    )
    build(
        "native",
        (),
        (
            "target/integration-artifacts/native",
            "target/integration-artifacts/client",
            "target/integration-artifacts/native.contract",
        ),
    )
    # Target-specific GUI contract declarations resolve the root project's aliases.
    build(
        "typescript",
        ("native", "client", "react-gui-authoring", "gui-stress-fixtures"),
        ("dist",),
        ("node", "npm"),
    )
    build("headless-client", ("native",), ("target/headless-client",), ("node", "npm"))
    build(
        "asset-rejection-tests",
        ("native", "client"),
        ("target/asset-rejection-tests",),
        ("node", "npm"),
    )
    build("react", ("client",), ("packages/ipp-react/dist",), ("node", "npm"))
    build(
        "asset-rejection-worker-tests",
        ("native", "client"),
        ("target/asset-rejection-worker-tests",),
        ("node", "npm"),
    )
    build(
        "react-gui-authoring",
        ("react", "native"),
        ("target/react-gui-authoring", "target/react-gui-contract"),
        ("node", "npm"),
    )
    build(
        "react-attached",
        ("react", "native"),
        ("target/react-attached",),
        ("node", "npm"),
    )
    build(
        "world-hosts", (), ("target/world-host-build",), ("node", "npm", "rust", "wasm")
    )
    build("dataset-fixtures", ("native",), ("target/datasets",), ("node", "npm"))
    build("plots-fixtures", ("native", "react"), ("target/plots",), ("node", "npm"))
    build("scaling-host", (), ("target/scaling-host-build",))
    build(
        "transport-fixtures", ("native",), ("target/multiplex-tests",), ("node", "npm")
    )
    build(
        "composed-query-fixtures",
        ("native",),
        ("target/composed-queries",),
        ("node", "npm"),
    )
    build("gui-motion-fixtures", (), ("target/gui-motion",), ("node", "npm"))
    # The harness's Node side is typed by the native build's generated client.
    build(
        "gui-composites-fixtures",
        ("native",),
        ("target/gui-composites",),
        ("node", "npm"),
    )
    # The skin lab's tokens are typed by the generated contract declarations.
    build(
        "gui-default-skin-fixtures",
        ("react-gui-authoring",),
        ("target/gui-default-skin",),
        ("node", "npm"),
    )
    build(
        "gui-stress-fixtures",
        ("react", "native"),
        ("target/gui-stress", "target/gui-stress-contract"),
        ("node", "npm"),
    )
    build(
        "worker-profiling-fixture",
        ("gallery-fixtures", "browser:render-instrumentation"),
        ("target/worker-profiling",),
        ("node", "npm"),
    )
    build("builtin-exporter", (), ("target/builtin-exporter",), ("rust",))
    build("render-fixtures", (), ("target/render-fixtures",), ("node", "npm"))
    for name in PROFILES["browser"]:
        build(
            f"browser:{name}",
            ("client",),
            (f"target/browser-build/{name}",),
            ("node", "npm", "rust", "wasm"),
        )
    build(
        "lifecycle-probes",
        (),
        ("target/browser-build/lifecycle-probes.js",),
        ("node", "npm"),
    )
    build(
        "gallery-assets",
        ("builtin-exporter",),
        ("target/gallery-assets",),
        ("node", "npm"),
    )
    build(
        "gallery",
        (
            "browser:render",
            "react",
            "gallery-assets",
            "gallery-gui-assets",
            "gallery-platformer-assets",
        ),
        ("target/gallery-build",),
        ("node", "npm"),
    )
    build(
        "gallery-site",
        ("gallery",),
        ("target/gallery-site",),
        ("node", "npm"),
    )
    build(
        "gallery-fixtures",
        ("gallery", "browser:headless"),
        ("target/gallery-fixtures",),
        ("node", "npm"),
    )
    build("react-fixtures", ("react",), ("target/react-build",), ("node", "npm"))
    build(
        "canvas-fixtures",
        ("react", "native"),
        ("target/canvas-build",),
        ("node", "npm"),
    )
    build(
        "textures",
        ("react", "builtin-exporter"),
        ("target/texture-build",),
        ("node", "npm"),
    )
    build(
        "shapes",
        ("react", "builtin-exporter"),
        ("target/shapes-build",),
        ("node", "npm"),
    )
    build(
        "font-assets",
        (),
        ("target/font-assets", "target/font-sources"),
        ("python-tools",),
    )
    build(
        "surface-assets",
        ("font-assets",),
        ("target/surface-assets",),
        ("python-tools",),
    )
    build(
        "gallery-gui-assets",
        ("font-assets",),
        ("target/gallery-gui-assets",),
        ("python-tools", "blender"),
    )
    build(
        "gallery-platformer-assets",
        ("browser:render",),
        ("target/gallery-platformer-assets",),
        ("python-tools", "blender", "node", "npm", "browser"),
    )
    build(
        "gallery-platformer-native-assets",
        ("native",),
        ("target/gallery-platformer-native-assets",),
        ("python-tools", "blender", "node", "npm"),
    )
    build(
        "surface-fixtures",
        ("react", "native", "surface-assets"),
        ("target/surface-build",),
        ("node", "npm"),
    )
    build(
        "surface-gui-fixtures",
        ("surface-fixtures",),
        ("target/surface-gui-build",),
        ("node", "npm"),
    )
    for name in ("gles-host", "gles-host-instrumentation"):
        build(name, (), (f"target/{name}",), ("node", "npm", "rust"))
    # The shared development Host command; `host start` builds gles-host itself.
    build("shared-host", (), ("target/shared-host",), ("node", "npm"))
    build("mesh-pose-fixtures", (), ("target/mesh-pose-build",), ("node", "npm"))
    build(
        "skinning-fixtures",
        ("builtin-exporter",),
        ("target/skinning-build",),
        ("rust",),
    )
    build(
        "blender-viewer",
        ("browser:render", "react"),
        ("target/blender-viewer",),
        ("node", "npm"),
    )
    # Test-only: the same viewer on the instrumentation runtime, for scenarios
    # that simulate context loss.
    build(
        "blender-viewer-instrumentation",
        ("browser:render-instrumentation", "react"),
        ("target/blender-viewer-instrumentation",),
        ("node", "npm"),
    )
    build(
        "blender-fixtures",
        ("react", "native"),
        ("target/blender-test",),
        ("node", "npm"),
    )
    # Its typecheck reads the native generated client.
    build(
        "blender-headless-fixtures",
        ("native",),
        (
            "target/blender-headless/scenario.js",
            "target/blender-headless/blender-headless.test.js",
            "target/blender-headless/blender-disk-headless.test.js",
        ),
        ("node", "npm"),
    )
    build(
        "blender-addon",
        (),
        ("target/blender/ipp_blender-0.1.0-linux-x64.zip",),
        ("blender-wheels",),
    )

    def check(
        name: str,
        command: tuple[str, ...],
        requirements: tuple[str, ...] = (),
        dependencies: tuple[str, ...] = (),
    ) -> None:
        add(Task(f"check:{name}", f"Check {name}", command, dependencies, requirements))

    add(
        Task(
            "check:distribution-sizes",
            "Measure current browser distributions without size ceilings",
            (
                sys.executable,
                "tools/measure_artifacts.py",
                *(
                    value
                    for profile in PROFILES["browser"]
                    for value in ("--profile", profile)
                ),
                "--output",
                "target/measurements/distribution-sizes.json",
            ),
            tuple(f"build:browser:{name}" for name in PROFILES["browser"]),
            ("rust", "node", "git"),
            ("target/measurements/distribution-sizes.json",),
        )
    )
    check("repository", (sys.executable, "tools/check_repo.py"), ("git",))
    check("catalog", operation("catalog"))
    check("workspace", (sys.executable, "tools/check_workspace.py"), ("rust",))
    check("diff", ("git", "diff", "--check", "HEAD"), ("git",))
    check(
        "typecheck",
        (node(), "node_modules/typescript/bin/tsc", "--noEmit"),
        ("node", "npm"),
        (
            "build:native",
            "build:client",
            "build:react-gui-authoring",
            "build:gui-stress-fixtures",
        ),
    )
    check("python-types", operation("python-types"), ("python-tools",))
    for language, requirements in {
        "js": ("git", "node", "npm"),
        "python": ("git", "python-tools"),
        "rust": ("rust",),
        "md": ("git", "node", "npm"),
    }.items():
        check(
            f"format-{language}", operation("format", language, "check"), requirements
        )
    # Production is the workspace default; all-features adds every instrumentation
    # axis, the WASM renderer and core's test-only oracle.
    for name, flags in (("default", ()), ("all-features", ("--all-features",))):
        check(
            f"clippy-{name}",
            (
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                *flags,
                "--locked",
                "--",
                "-D",
                "warnings",
            ),
            ("rust",),
        )
        check(
            f"wasm-{name}",
            (
                "cargo",
                "build",
                "-p",
                "ipp-wasm",
                "--target",
                "wasm32-unknown-unknown",
                *flags,
                "--locked",
            ),
            ("rust", "wasm"),
        )
    # One contract per host target; both come from the same compiled schema.
    check("contracts", operation("contracts"), ("node", "npm", "rust", "wasm"))
    check(
        "browser-identities",
        operation("browser-identities"),
        (),
        (
            "check:contracts",
            *(f"build:browser:{name}" for name in PROFILES["browser"]),
        ),
    )
    check(
        "contract-identities",
        operation("contract-identities"),
        ("rust",),
        ("check:contracts",),
    )

    for name, suite in SUITES.items():
        for entry in suite.get("commands", []):
            replacements = {
                "@python": sys.executable,
                "@development-python": development_python(),
                "@node": node(),
                "@blender": blender(),
            }
            if egl_directory:
                replacements["@egl"] = egl_directory
            command = tuple(replacements.get(part, part) for part in entry["command"])
            add(
                Task(
                    f"test:{name}:{entry['name']}",
                    suite["description"],
                    command,
                    tuple(f"build:{b}" for b in entry["builds"]),
                    tuple(entry["requirements"]),
                )
            )
        for file in suite.get("files", []):
            id_ = f"test:{file}"
            inputs = TEST_INPUTS[file]
            tasks[id_] = Task(
                id_,
                f"Run {file}",
                (node(), "--test", "--test-concurrency=1", file),
                tuple(f"build:{b}" for b in inputs["builds"]),
                tuple(inputs["requirements"]),
            )

    for record in GLES_CHECKS:
        replacements = {"@node": node()}
        if egl_directory:
            replacements["@egl"] = egl_directory
        add(
            Task(
                record["id"],
                f"Native GLES {record['id'].removeprefix('check:gles-')}",
                tuple(replacements.get(part, part) for part in record["command"]),
                tuple(record["dependencies"]),
                # Cargo examples need Rust; checks driving a generated client
                # through prepared hosts declare their own requirements instead.
                (
                    *(("rust",) if record["command"][0] == "cargo" else ()),
                    "gles",
                    *record.get("requirements", []),
                ),
            )
        )
    from .trace import register as register_traces

    register_traces(tasks)
    select(tasks, [])
    from .benchmark import register

    register(tasks)
    return tasks


def suite_ids(names: list[str]) -> list[str]:
    result: list[str] = []
    for name in names:
        if name not in SUITES:
            raise ValueError(f"Unknown suite: {name}. Use test --list.")
        if name == "contracts":
            result.extend(("check:contracts", "check:contract-identities"))
        result.extend(
            f"test:{name}:{entry['name']}" for entry in SUITES[name].get("commands", [])
        )
        result.extend(f"test:{file}" for file in SUITES[name].get("files", []))
    return list(dict.fromkeys(result))


def regression_ids(tasks: dict[str, Task], *, full: bool = False) -> list[str]:
    if full:
        return [name for name in tasks if name.startswith(("check:", "test:"))]
    return [
        "check:repository",
        "check:catalog",
        "check:diff",
        *[f"check:format-{name}" for name in ("js", "python", "rust", "md")],
        "check:python-types",
        "check:workspace",
        "check:typecheck",
        *suite_ids(["runner"]),
        "check:clippy-default",
        "test:rust:default",
        *suite_ids(["client", "native"]),
    ]


def regression_group_ids(names: list[str]) -> list[str]:
    result: list[str] = []
    for name in names:
        if name not in REGRESSION_GROUPS:
            raise ValueError(
                f"Unknown regression group: {name}. Use regression --list."
            )
        group = REGRESSION_GROUPS[name]
        result.extend(suite_ids(list(group.suites)))
        result.extend(group.steps)
    return list(dict.fromkeys(result))
