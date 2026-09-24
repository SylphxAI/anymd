#!/usr/bin/env python3
"""Public repository runner contract (platform decision 2026-09-24).

A self-hosted runner a public repository can reach runs fork pull requests'
code, so this repository runs on GitHub-hosted runners only: every static
`runs-on` (and every matrix runner/host value) must be a GitHub-hosted label,
and no selector may name `self-hosted` or a `sylphx-` profile.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

HOSTED = re.compile(r"^(?:ubuntu|macos|windows)-(?:latest|\d+(?:\.\d+)?(?:-arm)?)$", re.I)
FORBIDDEN = re.compile(r"self-hosted|sylphx-", re.I)
SELECTOR = re.compile(r"^\s*(?:-\s*)?(?:runs-on|runner|host|os)\s*:\s*(?P<value>[^#]*?)\s*(?:#.*)?$")


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
    workflows = sorted((*(root / ".github" / "workflows").glob("*.yml"), *(root / ".github" / "workflows").glob("*.yaml")))
    errors: list[str] = []
    for workflow in workflows:
        lines = workflow.read_text(encoding="utf-8").splitlines()
        for number, raw in enumerate(lines, 1):
            match = SELECTOR.match(raw)
            if not match:
                continue
            value = match.group("value").strip().strip("\"'")
            if not value or "${{" in value:
                continue
            if FORBIDDEN.search(value):
                errors.append(f"{workflow.relative_to(root)}:{number}: self-hosted runner in a public repository: {value}")
            elif raw.lstrip().startswith("runs-on") and not HOSTED.fullmatch(value):
                errors.append(f"{workflow.relative_to(root)}:{number}: not a GitHub-hosted runner label: {value}")
            if raw.rstrip().endswith("runs-on:"):
                continue
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"OK: {len(workflows)} workflow(s) run on GitHub-hosted runners (public repository)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
