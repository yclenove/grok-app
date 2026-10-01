#!/usr/bin/env python3
"""Serial mutation-sensitive gate runner.

Does not swallow failures, does not delete first-failure logs, does not
return 0 when a child fails. Records argv/cwd/exit/duration/hash.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def sha256_file(path: Path) -> tuple[str, int]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            h.update(chunk)
    return h.hexdigest(), size


def run_one(
    argv: list[str],
    cwd: Path,
    log_path: Path,
    timeout_s: int,
) -> dict:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.time()
    env = os.environ.copy()
    # Do not leak extra mutation env into child unless caller set them.
    proc = subprocess.run(
        argv,
        cwd=str(cwd),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout_s,
    )
    duration_ms = int((time.time() - started) * 1000)
    body = (
        f"argv={argv}\ncwd={cwd}\nexit={proc.returncode}\ndurationMs={duration_ms}\n"
        f"--- stdout ---\n{proc.stdout}\n--- stderr ---\n{proc.stderr}\n"
    )
    log_path.write_text(body, encoding="utf-8")
    digest, size = sha256_file(log_path)
    return {
        "argv": argv,
        "cwd": str(cwd),
        "exitCode": proc.returncode,
        "durationMs": duration_ms,
        "log": str(log_path),
        "sha256": digest,
        "bytes": size,
        "status": "passed" if proc.returncode == 0 else "failed",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--log-dir", required=True)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--stop-on-fail", action="store_true", default=True)
    parser.add_argument(
        "--json-spec",
        help="JSON list of {argv,cwd,name} objects",
    )
    args = parser.parse_args()
    spec = json.loads(Path(args.json_spec).read_text(encoding="utf-8"))
    log_dir = Path(args.log_dir)
    log_dir.mkdir(parents=True, exist_ok=True)
    results = []
    first_fail = None
    for i, item in enumerate(spec, start=1):
        argv = item["argv"]
        cwd = Path(item.get("cwd") or REPO)
        name = item.get("name") or f"step-{i:02d}"
        log_path = log_dir / f"{i:02d}-{name}.log"
        try:
            rec = run_one(argv, cwd, log_path, args.timeout)
        except subprocess.TimeoutExpired as e:
            rec = {
                "argv": argv,
                "cwd": str(cwd),
                "exitCode": 124,
                "durationMs": args.timeout * 1000,
                "log": str(log_path),
                "sha256": None,
                "bytes": 0,
                "status": "failed",
                "error": f"timeout: {e}",
            }
            log_path.write_text(f"timeout after {args.timeout}s\nargv={argv}\n", encoding="utf-8")
        rec["name"] = name
        rec["seq"] = i
        results.append(rec)
        if rec["exitCode"] != 0 and first_fail is None:
            first_fail = rec
            if args.stop_on_fail:
                break
    summary = {
        "ran": len(results),
        "failed": sum(1 for r in results if r["exitCode"] != 0),
        "firstFail": first_fail["name"] if first_fail else None,
        "results": results,
    }
    (log_dir / "summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8"
    )
    print(json.dumps({"ran": summary["ran"], "failed": summary["failed"], "firstFail": summary["firstFail"]}))
    return 0 if first_fail is None else 1


if __name__ == "__main__":
    raise SystemExit(main())
