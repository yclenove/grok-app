#!/usr/bin/env python3
"""Render the compatibility tables; this never runs or measures an App.

Uses only the Python standard library. This is a maintainer tool, not a
runtime dependency of gcu-eval. Rust integration tests independently check
the rendered sections without requiring Python on a runner.
"""

import argparse
import json
from pathlib import Path


def cell(value):
    return (str(value).replace("&", "&amp;").replace("<", "&lt;")
            .replace(">", "&gt;").replace("|", "&#124;")
            .replace("\r", "").replace("\n", "<br>"))


def optional(value):
    return cell("not_run" if value is None else value)


def render_tasks(catalog):
    lines = ["| Class | Fixed fixture task | External oracle | Fixture paths |",
             "| --- | --- | --- | --- |"]
    for entry in catalog["classes"]:
        fixtures = "<br>".join(f"[{cell(path)}](../{path})" for path in entry["fixtures"])
        lines.append(f'| {cell(entry["id"])} | '
                     f'[{cell(entry["task_id"])}](../{entry["task_path"]}) — '
                     f'{cell(entry["scenario"])} | {cell(entry["external_oracle"])} | {fixtures} |')
    return "\n".join(lines) + "\n"


def render_matrix(catalog, rows):
    count = len({row["app_id"] for row in rows})
    not_run = sum(row["level"] == "not_run" for row in rows)
    lines = [f'{count} Apps × {len(catalog["platforms"])} platforms = {len(rows)} rows. '
             f'{not_run} rows are `not_run`.', "",
             "| App | Platform | Level | App / OS / gcu versions | Tree quality | Background click | Background input | Known issues | Evidence | Date | Validation scope | Task |",
             "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"]
    classes = {entry["id"]: entry for entry in catalog["classes"]}
    for row in sorted(rows, key=lambda entry: (entry["app_id"], entry["platform"])):
        entry = classes[row["class"]]
        version = " / ".join(optional(row["versions"][key]) for key in ("app", "os", "gcu"))
        capabilities = row["capabilities"]
        evidence = "<br>".join(f'[{cell(path)}]({path if path.startswith("https://") else "../" + path})'
                               for path in row["evidence_paths"]) or "not_run"
        values = [cell(row["app"]), {"macos": "macOS", "windows": "Windows", "linux": "Linux"}[row["platform"]],
                  cell(row["level"]), version, optional(capabilities["tree_quality"]),
                  optional(capabilities["background_click"]), optional(capabilities["background_input"]),
                  "<br>".join(cell(issue) for issue in row["known_issues"]), evidence,
                  optional(row["verified_on"]), cell(row["validation_scope"]),
                  f'[{cell(entry["task_id"])}](../{entry["task_path"]})']
        lines.append("| " + " | ".join(values) + " |")
    return "\n".join(lines) + "\n"


def replace_section(document, name, content):
    start = f"<!-- compatibility:{name}:start -->\n"
    end = f"<!-- compatibility:{name}:end -->"
    if document.count(start) != 1 or document.count(end) != 1:
        raise ValueError(f"expected one {name} section")
    before, rest = document.split(start)
    _, after = rest.split(end)
    return before + start + content + end + after


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail on stale tables; do not write")
    args = parser.parse_args()
    workspace = Path(__file__).resolve().parents[3]
    catalog = json.loads((Path(__file__).parent / "catalog.json").read_text(encoding="utf-8"))
    rows = [row for path in catalog["matrices"]
            for row in json.loads((workspace / path).read_text(encoding="utf-8"))]
    path = workspace / "docs/compatibility.md"
    document = path.read_text(encoding="utf-8")
    rendered = replace_section(document, "tasks", render_tasks(catalog))
    rendered = replace_section(rendered, "matrix", render_matrix(catalog, rows))
    if args.check:
        if rendered != document:
            parser.exit(1, "compatibility tables are stale; run render.py\n")
        print("compatibility tables match data (not a desktop validation)")
    else:
        path.write_text(rendered, encoding="utf-8")


if __name__ == "__main__":
    main()
