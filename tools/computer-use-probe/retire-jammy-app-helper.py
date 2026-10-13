#!/usr/bin/env python3
"""Remove one completed task-owned helper, never the App, home or resources."""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

root = Path("/var/tmp/grok-cu-jammy-sdk-20261012-01")
target = root / "work/app-startup-source03-01/app-window"
evidence = Path(sys.argv[1]) if len(sys.argv) == 2 else Path("invalid")
if not evidence.is_absolute() or not evidence.is_dir() or evidence.resolve() != evidence:
    raise SystemExit("canonical evidence directory required")
report_path = evidence / "app-helper-cleanup.json"
if report_path.exists():
    raise SystemExit("cleanup evidence already exists")
for path in (target, *target.parents, evidence, *evidence.parents):
    if path.is_symlink():
        raise SystemExit(f"linked cleanup target/parent rejected: {path}")
for process in Path("/proc").iterdir():
    if not process.name.isdigit():
        continue
    try:
        actual_root = os.readlink(process / "root")
    except FileNotFoundError:
        continue
    if actual_root == str(root):
        raise SystemExit(f"private-root owner still live: {process.name}")
    try:
        actual_exe = os.readlink(process / "exe")
    except FileNotFoundError:
        continue
    if actual_exe == str(target):
        raise SystemExit(f"helper owner still live: {process.name}")
result = json.loads((target.parent / "results.json").read_text())
if not result.get("passed") or not result["owners"] or not all(
    owner["originalPidfdSignalled"] for owner in result["owners"]
):
    raise SystemExit("original App owner retirement is not proven")
metadata = target.lstat()
if not stat.S_ISREG(metadata.st_mode) or target.resolve(strict=True) != target:
    raise SystemExit("regular exact helper required")
digest = hashlib.sha256(target.read_bytes()).hexdigest()
if digest != "5507ac9eb0d60a53c2dc8330e53594a806733c057b178fe6d3a9a5bd041946e4":
    raise SystemExit("helper differs from the completed fixture")
before = os.statvfs(root).f_bavail * os.statvfs(root).f_frsize
target.unlink()
after = os.statvfs(root).f_bavail * os.statvfs(root).f_frsize
report = {
    "removed": str(target), "sha256": digest, "logicalBytes": metadata.st_size,
    "wslAvailableBytesBefore": before, "wslAvailableBytesAfter": after,
    "wslObservedAvailableDelta": after - before,
    "windowsPhysicalReclaimedBytes": None,
    "regenerate": "cc -Wall -Wextra -Werror -o /work/app-startup-source03-01/app-window "
                  "/work/app-startup-probe-01/jammy-app-window.c $(pkg-config --cflags --libs gtk+-3.0 x11)",
    "deferred": ["current App/SDK/toolchain/dependencies", "all home/log/map/screenshot/failure evidence"],
}
with report_path.open("x") as output:
    json.dump(report, output, indent=2)
    output.write("\n")
print(json.dumps(report, indent=2))
