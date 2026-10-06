"""Consistency of the task catalog, suite registries and CI pipeline invocations."""

from pathlib import Path
import re
import shlex

from .catalog import REGRESSION_GROUPS, catalog, regression_group_ids, regression_ids
from .cli import make_plan, parser
from .model import ROOT, select
from .product_reads import undeclared_product_reads
from .registries import GLES_CHECKS, PROFILES, SUITES, TEST_INPUTS


def validate_catalog() -> None:
    tasks = catalog("/validation/egl")
    for full in (False, True):
        select(tasks, regression_ids(tasks, full=full))
    grouped = regression_group_ids(list(REGRESSION_GROUPS))
    select(tasks, grouped)
    covered = set(regression_ids(tasks)) | set(grouped)
    unassigned = set(regression_ids(tasks, full=True)) - covered
    if unassigned:
        raise ValueError(
            f"Assign checks/suites to core or an on-demand regression group: {sorted(unassigned)}"
        )
    declared = {path for suite in SUITES.values() for path in suite.get("files", [])}
    if set(TEST_INPUTS) != declared:
        raise ValueError(
            f"Suite inputs differ from declared files: {sorted(set(TEST_INPUTS) ^ declared)}"
        )
    owners = [
        (f"Suite {name}", suite.get("sourceRoots", []))
        for name, suite in SUITES.items()
    ] + [(record["id"], record.get("sourceRoots", [])) for record in GLES_CHECKS]
    for owner, roots in owners:
        for root in roots:
            # A trailing slash owns a directory; otherwise the root is one file.
            if (
                not isinstance(root, str)
                or Path(root).is_absolute()
                or ".." in Path(root).parts
                or not (
                    (ROOT / root).is_dir()
                    if root.endswith("/")
                    else (ROOT / root).is_file()
                )
            ):
                raise ValueError(f"{owner} has invalid source root: {root!r}")
    for name, suite in SUITES.items():
        for path in suite.get("files", []):
            source = path.removeprefix("dist/")
            source = (
                source.removesuffix(".js") + ".ts"
                if path.startswith("dist/")
                else source
            )
            if not (ROOT / source).is_file():
                raise ValueError(f"Suite {name} references missing source: {source}")
    excluded = {
        f"test:{name}:{entry['name']}": tuple(entry.get("partitionExcludes", ()))
        for name, suite in SUITES.items()
        for entry in suite.get("commands", [])
    }
    undeclared = undeclared_product_reads(tasks, excluded)
    if undeclared:
        raise ValueError(
            "Tests read build products they do not depend on:\n" + "\n".join(undeclared)
        )
    workflow = (ROOT / ".github/workflows/gallery-pages.yml").read_text()
    # The Pages workflow uses explicit named invocations on single lines.
    count = 0
    for line in workflow.splitlines():
        command = line.strip().removeprefix("run: ")
        invocation = re.search(r"(?:^|\s)python3? tools/ipp\.py (.+)$", command)
        if invocation is None:
            continue
        args = parser().parse_args(shlex.split(invocation[1]))
        if args.command not in ("setup", "doctor"):
            make_plan(args)
        count += 1
    if count == 0:
        raise ValueError("CI must invoke the maintained Python pipeline")
    # Distributions vary only along the instrumentation and renderer axes.
    for name, profile in PROFILES["browser"].items():
        if set(profile) != {"features"} or not set(profile["features"]) <= {
            "render",
            "instrumentation",
        }:
            raise ValueError(f"Invalid browser profile: {name}")
    print(
        f"Validated {len(tasks)} tasks, {len(SUITES)} suites and {count} CI invocations."
    )
