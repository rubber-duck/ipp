"""Validated execution records and deterministic prerequisite selection."""

from dataclasses import asdict, dataclass
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]


@dataclass(frozen=True)
class Task:
    id: str
    description: str
    command: tuple[str, ...]
    dependencies: tuple[str, ...] = ()
    requirements: tuple[str, ...] = ()
    outputs: tuple[str, ...] = ()
    timeout: int = 1800
    interactive: bool = False
    mutates_source: bool = False

    def __post_init__(self) -> None:
        if not re.fullmatch(r"[a-zA-Z0-9:._/+-]+", self.id):
            raise ValueError(f"Invalid task ID: {self.id}")
        if not self.command or any(
            not isinstance(s, str) or not s for s in self.command
        ):
            raise ValueError(f"Invalid command: {self.id}")
        if self.timeout <= 0:
            raise ValueError(f"Invalid timeout: {self.id}")
        for output in self.outputs:
            if Path(output).is_absolute() or ".." in Path(output).parts:
                raise ValueError(f"Output must stay within the workspace: {output}")


@dataclass(frozen=True)
class Plan:
    command: str
    requested: tuple[str, ...]
    tasks: tuple[Task, ...]
    coverage: str = "focused"
    notes: tuple[str, ...] = ()

    def data(self) -> dict:
        return asdict(self)


def select(catalog: dict[str, Task], requested: list[str]) -> tuple[Task, ...]:
    """Validate the whole catalog, then return each selected prerequisite once."""
    complete: set[str] = set()
    visiting: set[str] = set()
    ordered: list[Task] = []

    def visit(name: str) -> None:
        if name in complete:
            return
        if name in visiting:
            raise ValueError(f"Dependency cycle at {name}")
        if name not in catalog:
            raise ValueError(f"Unknown task: {name}. Use --list.")
        task = catalog[name]
        if task.id != name:
            raise ValueError(f"Catalog key differs from task ID: {name}")
        visiting.add(name)
        for dependency in task.dependencies:
            visit(dependency)
        visiting.remove(name)
        complete.add(name)
        ordered.append(task)

    for name in catalog:
        visit(name)
    products: list[tuple[Path, str]] = []
    for task in catalog.values():
        for output in task.outputs:
            path = Path(output)
            for existing, owner in products:
                if path.is_relative_to(existing) or existing.is_relative_to(path):
                    raise ValueError(
                        f"Overlapping products: {task.id} ({output}) and {owner} ({existing})"
                    )
            products.append((path, task.id))
    complete.clear()
    ordered.clear()
    for name in requested:
        visit(name)
    return tuple(ordered)
