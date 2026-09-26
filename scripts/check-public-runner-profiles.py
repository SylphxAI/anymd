#!/usr/bin/env python3
"""Runner contract (owner standards/dx.md, 2026-09-26).

Every job runs on our own runners, public repository or not: a static
`runs-on` is a `sylphx-linux-<size>` class or the macOS pool
(`[self-hosted, sylphx, macos, <size>]`), and no selector names a
GitHub-hosted `ubuntu-*`, `windows-*` or `macos-*` label. A fork's pull
request reaches our runners only after a maintainer approves its run, and
each runner is one wiped VM per job.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

OWN = re.compile(r"^(?:sylphx-linux-(?:control|standard|large|xlarge|2xlarge)|\[self-hosted, sylphx, macos, \w+\])$")
HOSTED = re.compile(r"(?:^|[\s\[,'\"])(?:ubuntu|windows|macos)-", re.I)
SELECTOR = re.compile(r"^\s*(?:-\s*)?(?:runs-on|runner|host|os)\s*:\s*(?P<value>[^#]*?)\s*(?:#.*)?$")


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]
    workflows = sorted((*(root / ".github" / "workflows").glob("*.yml"), *(root / ".github" / "workflows").glob("*.yaml")))
    errors: list[str] = []
    for workflow in workflows:
        for number, raw in enumerate(workflow.read_text(encoding="utf-8").splitlines(), 1):
            match = SELECTOR.match(raw)
            if not match:
                continue
            value = match.group("value").strip().strip("\"'")
            if not value or "${{" in value:
                continue
            where = f"{workflow.relative_to(root)}:{number}"
            if HOSTED.search(value):
                errors.append(f"{where}: GitHub-hosted runner label: {value}")
            elif raw.lstrip().startswith("runs-on") and not OWN.fullmatch(value):
                errors.append(f"{where}: not one of our runner classes (sylphx-linux-<size>, or the macOS pool): {value}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"OK: {len(workflows)} workflow(s) run on our own runners")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
