"""Print a progress bar per phase of the roadmap, from the checkboxes in docs/PLAN.md.

A phase's tasks are its `- [ ]` / `- [x]` items whose label is a task id such as
`B.3` or `A.1c`. When a task is split into subtasks (`A.1` into `A.1a`...), only
the subtasks count. A phase whose heading says it is completed counts as done.

    python3 scripts/progress.py [--width N]
"""

from __future__ import annotations

import argparse
import re
from dataclasses import dataclass, field
from pathlib import Path

PLAN = Path(__file__).resolve().parents[1] / "docs" / "PLAN.md"
PHASE = re.compile(r"^### Fase (?P<id>[^.\s]+)\. (?P<title>.+)$")
TASK = re.compile(r"^\s*- \[(?P<mark>[ xX])\] (?P<id>[A-Z0-9']+\.\d+[a-z]?)\b")


@dataclass
class Phase:
    """One phase of the plan and the state of its tasks."""

    id: str
    title: str
    completed: bool
    tasks: dict[str, bool] = field(default_factory=dict[str, bool])

    def leaves(self) -> dict[str, bool]:
        """The tasks without subtasks: `A.1` is dropped when `A.1a` exists."""
        return {
            task: done
            for task, done in self.tasks.items()
            if not any(
                other != task and other.startswith(task) and other[len(task) :].isalpha()
                for other in self.tasks
            )
        }


def read_plan(path: Path) -> list[Phase]:
    """Return the phases of the plan, in order, with their tasks."""
    phases: list[Phase] = []
    current: Phase | None = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if heading := PHASE.match(line):
            title = heading["title"].split(" · ")[0].split(" — ")[0].strip()
            current = Phase(heading["id"], title, completed="completada" in heading["title"])
            phases.append(current)
        elif line.startswith("## "):
            current = None
        elif current is not None and (task := TASK.match(line)):
            current.tasks[task["id"]] = task["mark"] != " "
    return phases


def render(phases: list[Phase], width: int) -> str:
    """Draw one bar per phase, mark the current phase and add the total."""
    lines: list[str] = []
    done_total = count_total = 0
    current_seen = False
    title_width = max(len(phase.title) for phase in phases)
    for phase in phases:
        leaves = phase.leaves()
        done, count = sum(leaves.values()), len(leaves)
        if phase.completed and not count:
            bar, status = "█" * width, "done"
        else:
            filled = round(width * done / count) if count else 0
            bar, status = "█" * filled + "░" * (width - filled), f"{done}/{count}"
            done_total += done
            count_total += count
        marker = ""
        if not current_seen and not (phase.completed or (count and done == count)):
            marker, current_seen = "  ← now", True
        lines.append(f"{phase.id:<3} {phase.title:<{title_width}}  {bar} {status}{marker}")
    percent = 100 * done_total / count_total if count_total else 100.0
    lines.append(f"\nTotal: {done_total}/{count_total} tasks ({percent:.0f} %)")
    return "\n".join(lines)


def main() -> None:
    """Parse the arguments and print the bars."""
    parser = argparse.ArgumentParser(description="Progress bar per phase of docs/PLAN.md.")
    parser.add_argument("--width", type=int, default=20, help="bar width in characters")
    args = parser.parse_args()
    print(render(read_plan(PLAN), max(args.width, 1)))


if __name__ == "__main__":
    main()
