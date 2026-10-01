#!/usr/bin/env python3
"""D1 isolated mutation + shared --check; reports stay in ignored repo-local .run."""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SEED = REPO / "src-tauri" / "resources" / "computer-use" / "seed"
SIB = SEED / "playwright" / "loopback.mjs"
EXE = REPO / "src-tauri" / "target-cu-review" / "debug" / "cu-prepare-runtime.exe"
ISOLATED = REPO / "tools" / "computer-use-probe" / ".run" / "isolated-seeds"


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def clone_hardlink(src: Path, dst: Path) -> None:
    dst.mkdir(parents=True, exist_ok=True)
    for root, dirs, files in os.walk(src, followlinks=False):
        rel = Path(root).relative_to(src)
        dest_root = dst / rel
        dest_root.mkdir(parents=True, exist_ok=True)
        for d in dirs:
            p = Path(root) / d
            if p.is_symlink():
                raise RuntimeError(f"symlink dir {p}")
            (dest_root / d).mkdir(exist_ok=True)
        for f in files:
            s = Path(root) / f
            if s.is_symlink():
                raise RuntimeError(f"symlink file {s}")
            os.link(s, dest_root / f)


def run_check(seed: Path, timeout: int = 180) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            str(EXE),
            "--check",
            "--target",
            "x86_64-windows",
            "--repo",
            str(REPO),
            "--seed",
            str(seed),
        ],
        cwd=str(REPO / "src-tauri"),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )


def main() -> int:
    rounds = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    if not EXE.is_file():
        print("missing cu-prepare-runtime.exe", file=sys.stderr)
        return 2
    if not SIB.is_file():
        print("missing shared sibling", file=sys.stderr)
        return 2
    dest = ISOLATED.parent / "d1-serial-rounds" / (str(uuid.uuid4()) + ".json")
    dest.parent.mkdir(parents=True, exist_ok=True)
    baseline = sha(SIB)
    records = []
    for i in range(1, rounds + 1):
        t0 = time.time()
        before = sha(SIB)
        pre = run_check(SEED)
        if pre.returncode != 0:
            records.append(
                {
                    "round": i,
                    "step": "pre-check",
                    "exit": pre.returncode,
                    "stderr": pre.stderr[-2000:],
                }
            )
            dest.write_text(json.dumps(records, indent=2), encoding="utf-8")
            print(json.dumps(records[-1], indent=2))
            return 1
        isol_root = ISOLATED / str(uuid.uuid4())
        isol = isol_root / "seed"
        clone_hardlink(SEED, isol)
        os.remove(isol / "playwright" / "loopback.mjs")
        iso = run_check(isol)
        iso_ok = iso.returncode != 0 and "missing_sibling" in (iso.stderr or "")
        post = run_check(SEED)
        after = sha(SIB)
        shutil.rmtree(isol_root, ignore_errors=True)
        rec = {
            "round": i,
            "preCheckExit": pre.returncode,
            "isolatedExit": iso.returncode,
            "isolatedStderr": (iso.stderr or "")[-500:],
            "isolatedMissingSibling": iso_ok,
            "postCheckExit": post.returncode,
            "siblingUnchanged": before == after == baseline,
            "durationMs": int((time.time() - t0) * 1000),
        }
        records.append(rec)
        if (
            rec["preCheckExit"] != 0
            or not rec["isolatedMissingSibling"]
            or rec["postCheckExit"] != 0
            or not rec["siblingUnchanged"]
        ):
            print(json.dumps(rec, indent=2))
            dest.write_text(json.dumps(records, indent=2), encoding="utf-8")
            return 1
        print(f"round {i} ok {rec['durationMs']}ms")
    out = {
        "rounds": rounds,
        "baselineSibling": baseline,
        "finalSibling": sha(SIB),
        "records": records,
        "status": "passed",
    }
    dest.write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(
        json.dumps(
            {
                "rounds": rounds,
                "status": "passed",
                "baselineSibling": baseline,
                "output": str(dest),
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
