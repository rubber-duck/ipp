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
            "react",
        ),
    ),
    "browser": RegressionGroup(
        "Chromium worker/WASM transport and lifecycle", ("browser",)
    ),
    "rendering": RegressionGroup(
        "WebGL frames, cameras, geometry, materials, lighting, deformation and resource recovery",
        (
            "render",
            "canvas",
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
        ),
    ),
    "gui": RegressionGroup(
        "Surface/GUI state, input, retained rendering and Surface cache correctness",
        ("surfaces", "gui", "retained-gui", "surface-cache"),
    ),
    "gallery": RegressionGroup(
        "Published gallery and interactive GUI, camera, particle and platformer demos",
        (
            "gallery-site",
            "gallery-gui",
            "gallery-gui-camera",
            "gallery-particles",
            "gallery-platformer",
        ),
    ),
    "blender": RegressionGroup(
        "Blender packaging, exports, streaming and browser presentation",
        ("blender", "particles-blender"),
    ),
    "gles": RegressionGroup(
        "All native GLES frame scenarios; requires configured EGL/GLES libraries",
        steps=tuple(record["id"] for record in GLES_CHECKS),
    ),
    "matrix": RegressionGroup(
        "Minimal/expanded Rust, WASM builds, target contracts and browser distribution identities",
        ("contracts",),
        (
            "test:rust:minimal",
            "test:rust:expanded",
            "check:clippy-minimal",
            "check:clippy-expanded",
            "check:wasm-default",
            "check:wasm-minimal",
            "check:wasm-expanded",
            "check:wasm-size",
            "check:browser-identities",
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
    build(
        "native",
        (),
        (
            "target/integration-artifacts/native",
            "target/integration-artifacts/client",
            "target/integration-artifacts/native.contract",
        ),
    )
    build("typescript", ("native", "client"), ("dist",))
    build("react", ("client",), ("packages/ipp-react/dist",), ("node", "npm"))
    build(
        "world-hosts", (), ("target/world-host-build",), ("node", "npm", "rust", "wasm")
    )
    build("scaling-host", (), ("target/scaling-host-build",))
    build("builtin-exporter", (), ("target/builtin-exporter",), ("rust",))
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
        "gallery",
        (
            "browser:render-expanded",
            "react",
            "builtin-exporter",
            "gallery-gui-assets",
            "gallery-platformer-assets",
        ),
        ("target/gallery-build",),
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
    )
    build("react-fixtures", ("react",), ("target/react-build",), ("node", "npm"))
    build("canvas-fixtures", ("react",), ("target/canvas-build",), ("node", "npm"))
    build("textures", ("react", "builtin-exporter"), ("target/texture-build",))
    build("shapes", ("react", "builtin-exporter"), ("target/shapes-build",))
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
        ("browser:render-expanded",),
        ("target/gallery-platformer-assets",),
        ("python-tools", "blender", "node", "npm", "browser"),
    )
    build(
        "surface-fixtures",
        ("react", "surface-assets"),
        ("target/surface-build",),
        ("node", "npm"),
    )
    build("surface-host", (), ("target/surface-host",), ("node", "npm", "rust"))
    build("gui-host", (), ("target/gui-host",), ("node", "npm", "rust"))
    build("gles-hosts", (), ("target/gles-host",), ("node", "npm", "rust"))
    build("mesh-pose-fixtures", (), ("target/mesh-pose-build",), ("node", "npm"))
    build(
        "skinning-fixtures",
        ("builtin-exporter",),
        ("target/skinning-build",),
        ("rust",),
    )
    build(
        "blender-viewer",
        ("browser:render-expanded", "react"),
        ("target/blender-viewer",),
    )
    build(
        "blender-fixtures",
        ("react",),
        ("target/blender-test/blender-fixture.js",),
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

    check("repository", (sys.executable, "tools/check_repo.py"), ("git",))
    check("catalog", operation("catalog"))
    check("workspace", (sys.executable, "tools/check_workspace.py"), ("rust",))
    check("diff", ("git", "diff", "--check", "HEAD"), ("git",))
    check(
        "typecheck",
        (node(), "node_modules/typescript/bin/tsc", "--noEmit"),
        ("node", "npm"),
        ("build:native", "build:client"),
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
    for name, flags in (
        ("default", ()),
        ("minimal", ("--no-default-features",)),
        ("expanded", ("--all-features",)),
    ):
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
    check(
        "wasm-size",
        (
            "cargo",
            "build",
            "-p",
            "ipp-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--profile",
            "release-small",
            "--no-default-features",
            "--features",
            "render",
            "--locked",
        ),
        ("rust", "wasm"),
    )
    for profile in PROFILES["contracts"]:
        check(
            f"contracts:{profile}",
            operation("contracts", profile),
            ("node", "npm", "rust", "wasm"),
        )
    check(
        "browser-identities",
        operation("browser-identities"),
        (),
        tuple(f"build:browser:{name}" for name in PROFILES["browser"]),
    )
    check(
        "contract-identities",
        operation("contract-identities"),
        (),
        tuple(f"check:contracts:{name}" for name in PROFILES["contracts"]),
    )

    for name, suite in SUITES.items():
        for entry in suite.get("commands", []):
            replacements = {
                "@python": sys.executable,
                "@development-python": development_python(),
                "@node": node(),
                "@blender": blender(),
            }
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
                # Checks driving a generated client also declare its requirements.
                ("rust", "gles", *record.get("requirements", [])),
            )
        )
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
            result.extend(
                f"check:contracts:{profile}" for profile in PROFILES["contracts"]
            )
            result.append("check:contract-identities")
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
