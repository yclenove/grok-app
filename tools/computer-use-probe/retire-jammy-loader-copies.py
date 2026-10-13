#!/usr/bin/env python3
"""Retire only completed loader case copies; preserve code, SDKs and all evidence."""
import json
import os
from pathlib import Path
import re
import stat
import sys


def no_links(path):
    for item in (path, *path.parents):
        if item.is_symlink():
            raise RuntimeError(f"Linked cleanup target/parent: {item}")


def root_owners(root):
    owners = []
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            actual = os.readlink(process / "root")
        except FileNotFoundError:
            continue
        except OSError as error:
            raise RuntimeError(f"Cannot verify process root for {process.name}: {error}") from error
        if actual == str(root):
            owners.append(int(process.name))
    return owners


def retire(root, evidence):
    if not re.fullmatch(r"/var/tmp/grok-cu-jammy-sdk-[A-Za-z0-9-]+", str(root)):
        raise RuntimeError("Task-owned Jammy root required")
    for path in (root, evidence):
        no_links(path)
        if not path.is_absolute() or path.resolve(strict=True) != path or not path.is_dir():
            raise RuntimeError("Canonical existing root/evidence directory required")
    report_path = evidence / "cleanup.json"
    journal_path = evidence / "removals.jsonl"
    if report_path.exists() or journal_path.exists():
        raise RuntimeError("Cleanup evidence must be new")
    owners = root_owners(root)
    before = os.statvfs(root).f_bavail * os.statvfs(root).f_frsize
    report = {"scope": "retired-product-loader-case-copies-01-03", "logicalBytesRemoved": 0,
              "filesRemoved": 0, "deferred": [], "liveRootOwners": owners,
              "windowsPhysicalReclaimedBytes": None,
              "regenerate": "run-jammy-product-loader.sh with the retained frozen inputs/SDK/toolchain"}
    if owners:
        report["deferred"].append("All case copies: original private-root owner still live")
    else:
        with journal_path.open("x") as journal:
            for batch in ("product-loader-01", "product-loader-02", "product-loader-03"):
                acceptance = root / "work" / batch / "acceptance"
                no_links(acceptance)
                result = json.loads((acceptance / "results.json").read_text())
                if not result.get("passed") or result.get("scope") != "actual-product-loader-module-on-jammy":
                    raise RuntimeError("Case batch has no completed passing evidence")
                for index in range(len(result["cases"])):
                    target = acceptance / f"case-{index:02d}"
                    no_links(target)
                    if target.resolve(strict=True).parent != acceptance or not target.is_dir():
                        raise RuntimeError("Case copy escapes its exact acceptance directory")
                    files, directories, unsafe = [], [target], []
                    for directory, children, names in os.walk(target, followlinks=False):
                        for name in (*children, *names):
                            path = Path(directory) / name
                            mode = path.lstat().st_mode
                            if stat.S_ISREG(mode):
                                files.append((path, path.lstat()))
                            elif stat.S_ISDIR(mode):
                                directories.append(path)
                            else:
                                unsafe.append(str(path))
                    if unsafe:
                        report["deferred"].append({"target": str(target), "reason": "Link/special fixture retained", "entries": unsafe})
                        continue
                    if root_owners(root):
                        raise RuntimeError("Private root gained a live owner; cleanup stopped")
                    for path, original in files:
                        no_links(path)
                        current = path.lstat()
                        if not stat.S_ISREG(current.st_mode) or (current.st_dev, current.st_ino, current.st_size) != (original.st_dev, original.st_ino, original.st_size):
                            raise RuntimeError("Case file changed during cleanup")
                        entry = {"path": str(path), "bytes": current.st_size, "event": "removing"}
                        journal.write(json.dumps(entry) + "\n")
                        journal.flush()
                        os.fsync(journal.fileno())
                        path.unlink()
                        report["filesRemoved"] += 1
                        report["logicalBytesRemoved"] += current.st_size
                    for path in sorted(directories, key=lambda p: len(p.parts), reverse=True):
                        path.rmdir()
    after = os.statvfs(root).f_bavail * os.statvfs(root).f_frsize
    report["wslAvailableBytesBefore"] = before
    report["wslAvailableBytesAfter"] = after
    report["wslObservedAvailableDelta"] = after - before
    with report_path.open("x") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps(report))


if __name__ == "__main__":
    retire(*(Path(value) for value in sys.argv[1:]))
