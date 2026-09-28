#!/usr/bin/env python3
"""Print the OmniDocBench v1.6 headline scores from the evaluator's metric_result.json.

Overall is the leaderboard formula: ((1 - text edit distance) * 100 + table TEDS + formula CDM) / 3.
"""

import json
import sys


def first_number(node):
    if isinstance(node, (int, float)):
        return float(node)
    if isinstance(node, dict):
        for value in node.values():
            found = first_number(value)
            if found is not None:
                return found
    return None


def scores(path):
    data = json.load(open(path))

    def get(section, metric):
        return first_number(data[section]["all"][metric])

    text = get("text_block", "Edit_dist")
    cdm = get("display_formula", "CDM")
    teds = get("table", "TEDS")
    teds_s = data["table"]["all"].get("TEDS_structure_only")
    order = get("reading_order", "Edit_dist")
    to_pct = lambda v: v * 100 if v is not None and v <= 1 else v
    cdm, teds = to_pct(cdm), to_pct(teds)
    return {
        "overall": ((1 - text) * 100 + teds + cdm) / 3,
        "text_edit": text,
        "formula_cdm": cdm,
        "table_teds": teds,
        "table_teds_structure_only": to_pct(first_number(teds_s)),
        "reading_order_edit": order,
        "table_edit": get("table", "Edit_dist"),
        "formula_edit": get("display_formula", "Edit_dist"),
    }


if __name__ == "__main__":
    result = scores(sys.argv[1])
    print(json.dumps(result, indent=2))
