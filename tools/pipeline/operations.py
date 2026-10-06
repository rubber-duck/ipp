"""Leaf operations invoked by the supervised executor, without a second task graph."""

import json
import sys

from . import contracts
from .builds import build, verify_browser_identities
from .catalog_validation import validate_catalog
from .environment import development_python
from .formatting import format_source
from .processes import run


def main(args: list[str]) -> None:
    operation, *remaining = args
    if operation == "build":
        build(remaining[0])
    elif operation == "benchmark-build":
        from .benchmark import build_browser, build_gui_native, build_native

        if remaining[0] == "browser":
            build_browser()
        elif remaining[0].startswith("gui-native"):
            build_gui_native(remaining[0] == "gui-native-instrumented")
        else:
            build_native(remaining[0] == "native-instrumented")
    elif operation == "gui-paired":
        from .gui_comparison import compare

        compare(json.loads(remaining[0]))
    elif operation == "chart-benchmark-build":
        from .chart_benchmark import build as build_charts

        build_charts()
    elif operation == "chart-diagnostic-build":
        from .chart_benchmark import build_diagnostic

        build_diagnostic(remaining[0], remaining[1] == "allocations")
    elif operation == "chart-diagnostic":
        from .chart_benchmark import diagnostic

        diagnostic(json.loads(remaining[0]))
    elif operation == "benchmark":
        from .benchmark import scene

        scene(json.loads(remaining[0]))
    elif operation == "format":
        format_source(remaining[0], remaining[1], remaining[2:])
    elif operation == "python-types":
        run([development_python(), "-m", "mypy", "--config-file", "mypy.ini"])
        if sys.platform != "win32":
            run(
                [
                    development_python(),
                    "-m",
                    "mypy",
                    "--config-file",
                    "mypy.ini",
                    "--platform",
                    "win32",
                ]
            )
    elif operation == "catalog":
        validate_catalog()
    elif operation == "contracts":
        contracts.main()
    elif operation == "browser-identities":
        verify_browser_identities()
    elif operation == "contract-identities":
        contracts.identities()
    elif operation == "serve":
        from .server import serve

        serve(remaining[0], int(remaining[1]) if len(remaining) > 1 else None)
    elif operation == "setup":
        from .setup import setup

        setup(remaining)
    elif operation == "blender-addon":
        from .blender_addon import main as blender_addon

        blender_addon(remaining)
    elif operation == "measure":
        from .measure import main as measure

        measure(remaining)
    else:
        raise ValueError(f"Unknown operation: {operation}")
