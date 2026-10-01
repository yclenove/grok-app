#!/usr/bin/env python3
"""Finalization evidence protocol (Python 3.8). Ignored under tools/computer-use-probe/.run/."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import socket
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

SCHEMA_VERSION = 1
REPO = Path(__file__).resolve().parents[2]
EVIDENCE_PARENT = REPO / "tools" / "computer-use-probe" / ".run" / "finalization"
SCRATCH_DEFAULT = EVIDENCE_PARENT / "scratch"
BASELINE_HEAD = "30757366a739ec9aaf0ccc95bbb3efe19a067aa9"
EXPECTED_BRANCH = "feat/computer-use-implementation"
SOURCE_SUFFIXES = {
    ".rs",
    ".ts",
    ".tsx",
    ".js",
    ".mjs",
    ".cjs",
    ".json",
    ".md",
    ".css",
    ".toml",
    ".yml",
    ".yaml",
    ".html",
    ".lock",
    ".sh",
    ".ps1",
    ".py",
    ".svg",
    ".xml",
    ".conf",
    ".nsis",
    ".nsh",
}
SECRET_NAMES = {"auth.json", "secrets.json", ".env"}
ATOMIC_STATUSES = (
    "not_started",
    "in_progress",
    "passed",
    "failed",
    "blocked_external",
    "invalidated",
    "not_run",
)
APP_EXE = REPO / "src-tauri" / "target-cu-review" / "debug" / "grok-app.exe"
STUB_EXE = (
    REPO
    / "tools"
    / "computer-use-probe"
    / "cu-acp-stub"
    / "target"
    / "debug"
    / "cu-acp-stub.exe"
)
OLD_RUNTIME = (
    REPO
    / "tools"
    / "computer-use-probe"
    / ".run"
    / "48h"
    / "20260911T080635Z-seven"
    / "d12-d6-home"
    / "computer-use"
    / "runtime"
)


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def local_now() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path, max_bytes=None):
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            if max_bytes is not None and size > max_bytes:
                h.update(chunk)
                rest = path.stat().st_size
                return "truncated:" + h.hexdigest(), rest
            h.update(chunk)
    return h.hexdigest(), size


def run_git(args, cwd=REPO):
    return subprocess.run(
        ["git"] + list(args),
        cwd=str(cwd),
        text=True,
        capture_output=True,
        check=False,
        encoding="utf-8",
        errors="replace",
    )


def check_ignore(rel: str) -> bool:
    p = subprocess.run(
        ["git", "check-ignore", "-q", rel],
        cwd=str(REPO),
        check=False,
    )
    return p.returncode == 0


def atomic_write_json(path: Path, obj: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    payload = json.dumps(obj, indent=2, ensure_ascii=False) + "\n"
    data = payload.encode("utf-8")
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
    fd = os.open(str(tmp), flags, 0o644)
    try:
        os.write(fd, data)
        os.fsync(fd)
    finally:
        os.close(fd)
    os.replace(str(tmp), str(path))


def append_jsonl(path: Path, obj: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    line = json.dumps(obj, ensure_ascii=False, separators=(",", ":")) + "\n"
    with path.open("a", encoding="utf-8", newline="\n") as f:
        f.write(line)
        f.flush()
        os.fsync(f.fileno())


def next_seq(manifest: Path) -> int:
    if not manifest.is_file():
        return 1
    n = 0
    with manifest.open("r", encoding="utf-8") as f:
        for line in f:
            if line.strip():
                n += 1
    return n + 1


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def evidence_root() -> Path:
    env = os.environ.get("GROK_CU_FINALIZATION_ROOT")
    if env:
        return Path(env)
    marker = EVIDENCE_PARENT / "CURRENT_RUN"
    if marker.is_file():
        run_id = marker.read_text(encoding="utf-8").strip()
        return EVIDENCE_PARENT / run_id
    raise SystemExit("no evidence root; run f0-init first")


def make_run_id() -> str:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    host = socket.gethostname().split(".")[0].lower()
    return stamp + "-" + host


def dir_size_bounded(path: Path, limit_files: int = 20000) -> Dict[str, Any]:
    if not path.exists():
        return {"exists": False}
    files = 0
    bytes_ = 0
    truncated = False
    if path.is_file():
        return {"exists": True, "files": 1, "bytes": path.stat().st_size}
    for root, dirs, names in os.walk(path):
        dirs[:] = [d for d in dirs if not os.path.islink(os.path.join(root, d))]
        for name in names:
            files += 1
            if files > limit_files:
                truncated = True
                break
            fp = Path(root) / name
            try:
                st = fp.lstat()
            except OSError:
                continue
            if stat.S_ISREG(st.st_mode):
                bytes_ += st.st_size
        if truncated:
            break
    return {
        "exists": True,
        "files": files,
        "bytes": bytes_,
        "truncated": truncated,
    }


def classify_untracked(rel: str) -> bool:
    low = rel.replace("\\", "/").lower()
    skip_bits = (".run/", "/seed/", "/node_modules/", "/target/", "target-cu")
    for bit in skip_bits:
        if bit in low:
            return False
    suffix = Path(rel).suffix.lower()
    if suffix in SOURCE_SUFFIXES:
        return True
    name = Path(rel).name.lower()
    return name in {"dockerfile", "makefile", "license", "notice"}


def fingerprint(repo: Path = REPO) -> Dict[str, Any]:
    head = run_git(["rev-parse", "HEAD"]).stdout.strip()
    branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"]).stdout.strip()
    diff = run_git(["diff", "--binary", "HEAD"])
    tracked_diff_sha = sha256_bytes((diff.stdout or "").encode("utf-8", "replace"))
    untracked = run_git(["ls-files", "--others", "--exclude-standard"])
    files = [
        ln.strip().replace("\\", "/")
        for ln in (untracked.stdout or "").splitlines()
        if ln.strip()
    ]
    included = []
    skipped = []
    merkle = hashlib.sha256()
    for rel in sorted(files):
        path = repo / rel
        if Path(rel).name.lower() in SECRET_NAMES:
            skipped.append({"path": rel, "reason": "secret_name"})
            continue
        if not classify_untracked(rel):
            skipped.append({"path": rel, "reason": "excluded_generated_or_binary"})
            continue
        if not path.is_file():
            skipped.append({"path": rel, "reason": "not_file"})
            continue
        digest, size = sha256_file(path)
        rec = {"path": rel, "sha256": digest, "bytes": size}
        included.append(rec)
        merkle.update(("%s:%s:%s\n" % (rel, digest, size)).encode("utf-8"))
    lock_paths = [
        "package.json",
        "pnpm-lock.yaml",
        "src-tauri/Cargo.lock",
        "src-tauri/Cargo.toml",
        "src-tauri/resources/computer-use/windows-x64.lock.json",
        "src-tauri/resources/computer-use/chromium-lock.json",
    ]
    locks = {}
    lock_merkle = hashlib.sha256()
    for rel in lock_paths:
        p = repo / rel
        if p.is_file():
            digest, size = sha256_file(p)
            locks[rel] = {"sha256": digest, "bytes": size}
            lock_merkle.update(("%s:%s\n" % (rel, digest)).encode("utf-8"))
        else:
            locks[rel] = {"missing": True}
            lock_merkle.update(("%s:missing\n" % rel).encode("utf-8"))
    code_fp = sha256_bytes(
        "\n".join(
            [
                "head:" + head,
                "branch:" + branch,
                "tracked_diff:" + tracked_diff_sha,
                "untracked:" + merkle.hexdigest(),
                "locks:" + lock_merkle.hexdigest(),
            ]
        ).encode("utf-8")
    )
    return {
        "schemaVersion": SCHEMA_VERSION,
        "computedAt": utc_now(),
        "head": head,
        "branch": branch,
        "trackedDiffSha256": tracked_diff_sha,
        "trackedDiffBytes": len((diff.stdout or "").encode("utf-8", "replace")),
        "untrackedSourceCount": len(included),
        "untrackedSkippedCount": len(skipped),
        "untrackedMerkle": merkle.hexdigest(),
        "lockDigest": lock_merkle.hexdigest(),
        "locks": locks,
        "codeFingerprint": code_fp,
        "includedSample": included[:20],
        "includedCount": len(included),
        "skippedSample": skipped[:20],
        "included": included,
    }


def write_log(root: Path, rel: str, text: str) -> Dict[str, Any]:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    data = text.encode("utf-8")
    path.write_bytes(data)
    digest, size = sha256_file(path)
    return {"log": rel.replace("\\", "/"), "sha256": digest, "bytes": size}


def write_bytes(root: Path, rel: str, data: bytes) -> Dict[str, Any]:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    digest, size = sha256_file(path)
    return {"log": rel.replace("\\", "/"), "sha256": digest, "bytes": size}


def record(root, **kwargs):
    status = kwargs.get("status")
    if status not in ATOMIC_STATUSES:
        raise SystemExit("invalid atomic status: %s" % status)
    seq = next_seq(root / "manifest.jsonl")
    rec = {
        "schemaVersion": SCHEMA_VERSION,
        "seq": seq,
        "phase": kwargs["phase"],
        "batch": kwargs["batch"],
        "scenario": kwargs.get("scenario", ""),
        "kind": kwargs["kind"],
        "argvRedacted": kwargs["argv"],
        "cwd": kwargs["cwd"],
        "startedAt": kwargs["started_at"],
        "endedAt": kwargs["ended_at"],
        "durationMs": kwargs["duration_ms"],
        "exitCode": kwargs["exit_code"],
        "status": status,
        "evidenceLevel": kwargs["evidence_level"],
        "log": kwargs["log_rel"],
        "sha256": kwargs["sha256"],
        "bytes": kwargs["bytes_"],
        "codeFingerprint": kwargs["fingerprint_hex"],
        "postconditions": kwargs["postconditions"],
    }
    append_jsonl(root / "manifest.jsonl", rec)
    return rec


def default_batches() -> Dict[str, Any]:
    ids = [
        "F0",
        "F1",
        "F2",
        "F3",
        "F4",
        "F5",
        "F6",
        "F7",
        "F8",
        "F9",
        "F10",
        "F11",
        "F12",
        "F13",
        "F14",
    ]
    out = {}
    for bid in ids:
        out[bid] = {"status": "not_started", "sub": {}, "highestEvidence": None}
    out["F0"]["status"] = "in_progress"
    return out


def update_state(root: Path, mutator) -> Dict[str, Any]:
    path = root / "state.json"
    state = load_json(path)
    mutator(state)
    state["updatedAt"] = utc_now()
    atomic_write_json(path, state)
    return state


def disk_free():
    rows = []
    import ctypes

    for letter in "CDEFHYZ":
        p = Path("%s:/" % letter)
        if not p.exists():
            continue
        free = ctypes.c_ulonglong()
        total = ctypes.c_ulonglong()
        ctypes.windll.kernel32.GetDiskFreeSpaceExW(
            ctypes.c_wchar_p("%s:\\" % letter),
            None,
            ctypes.byref(total),
            ctypes.byref(free),
        )
        rows.append({"drive": letter, "free": free.value, "total": total.value})
    return rows


def line_counts():
    files = {
        "App.tsx": REPO / "src" / "App.tsx",
        "AppWorkbench.tsx": REPO / "src" / "app" / "AppWorkbench.tsx",
    }
    out = {}
    total = 0
    for name, path in files.items():
        if path.is_file():
            n = len(path.read_text(encoding="utf-8", errors="replace").splitlines())
            out[name] = n
            total += n
        else:
            out[name] = None
    out["combined"] = total
    return out


def writer_snapshot():
    cargo = []
    try:
        out = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq cargo.exe", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
            check=False,
        )
        cargo.append({"tasklist_cargo": (out.stdout or "")[:800]})
        out2 = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq grok-app.exe", "/FO", "CSV", "/NH"],
            capture_output=True,
            text=True,
            check=False,
        )
        cargo.append({"tasklist_grok_app": (out2.stdout or "")[:800]})
    except OSError as e:
        cargo.append({"error": str(e)})
    return {
        "pid": os.getpid(),
        "ppid": os.getppid(),
        "cwd": str(Path.cwd()),
        "hostname": socket.gethostname(),
        "cargoRustc": cargo,
    }


def copy_to_scratch(src: Path, dest_dir: Path) -> None:
    dest_dir.mkdir(parents=True, exist_ok=True)
    if src.is_dir():
        target = dest_dir / src.name
        if target.exists():
            shutil.rmtree(target, ignore_errors=True)
        shutil.copytree(str(src), str(target))
    else:
        shutil.copy2(str(src), str(dest_dir / src.name))


def f0_init() -> Path:
    probe = "tools/computer-use-probe/.run/"
    probe_ok = check_ignore(probe)
    if not probe_ok:
        raise SystemExit("git check-ignore failed for %s" % probe)
    run_id = make_run_id()
    root = EVIDENCE_PARENT / run_id
    for sub in ("baseline", "checkpoints", "failures", "logs", "metrics", "reports"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    sample = "tools/computer-use-probe/.run/finalization/%s/owner.json" % run_id
    nested_ok = check_ignore(sample)
    ignore_log = write_log(
        root,
        "logs/F0.0-check-ignore.log",
        "probe_run=%s nested_owner=%s sample=%s\n" % (probe_ok, nested_ok, sample),
    )
    if not nested_ok:
        raise SystemExit("git check-ignore failed for nested evidence path")
    (EVIDENCE_PARENT / "CURRENT_RUN").write_text(run_id + "\n", encoding="utf-8")
    git_status = run_git(["status", "--short", "--branch"])
    tracked = run_git(["diff", "--name-only", "--diff-filter=ACDMR", "HEAD"])
    untracked = run_git(["ls-files", "--others", "--exclude-standard"])
    ignored = run_git(["ls-files", "--others", "-i", "--exclude-standard"])
    tracked_n = len([l for l in (tracked.stdout or "").splitlines() if l.strip()])
    untracked_n = len([l for l in (untracked.stdout or "").splitlines() if l.strip()])
    ignored_n = len([l for l in (ignored.stdout or "").splitlines() if l.strip()])
    head = run_git(["rev-parse", "HEAD"]).stdout.strip()
    branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"]).stdout.strip()
    canonical = str(REPO)
    top_level = run_git(["rev-parse", "--show-toplevel"])
    identity_ok = (
        top_level.returncode == 0
        and bool(top_level.stdout.strip())
        and Path(top_level.stdout.strip()).resolve() == REPO
        and branch == EXPECTED_BRANCH
        and head == BASELINE_HEAD
    )
    owner = {
        "runId": run_id,
        "startedAtUtc": utc_now(),
        "startedAtLocal": local_now(),
        "repoCanonicalPath": canonical,
        "branch": branch,
        "baselineHead": BASELINE_HEAD,
        "actualHead": head,
        "writerPid": os.getpid(),
        "writerPpid": os.getppid(),
        "hostname": socket.gethostname(),
        "os": platform.platform(),
        "arch": platform.machine(),
        "evidenceSchemaVersion": SCHEMA_VERSION,
        "cleanupAllowlist": [
            str(root.resolve()),
            str(SCRATCH_DEFAULT.resolve()),
        ],
        "identityOk": identity_ok,
        "sessionNote": "finalization F0-F14; do not reuse D12/D13/D14 passed",
    }
    atomic_write_json(root / "owner.json", owner)
    state = {
        "schemaVersion": SCHEMA_VERSION,
        "runId": run_id,
        "phase": "F0",
        "batches": default_batches(),
        "fingerprint": None,
        "createdAt": utc_now(),
        "updatedAt": utc_now(),
        "identityOk": identity_ok,
        "rollup": "partial",
        "oldD12D13D14": "invalidated",
        "notes": [
            "Do not copy old D13/D14 passed",
            "atomic statuses only; partial is rollup-only",
        ],
    }
    atomic_write_json(root / "state.json", state)
    (root / "manifest.jsonl").write_text("", encoding="utf-8")
    record(
        root,
        phase="F0",
        batch="F0.0",
        scenario="check-ignore",
        kind="command",
        argv=["git", "check-ignore", "-q", probe],
        cwd=str(REPO),
        started_at=utc_now(),
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0,
        status="passed",
        evidence_level="E0",
        log_rel=ignore_log["log"],
        sha256=ignore_log["sha256"],
        bytes_=ignore_log["bytes"],
        fingerprint_hex="pending",
        postconditions=["git_check_ignore=true", "nested_owner_ignored=true"],
    )

    started = utc_now()
    t0 = time.time()
    status_log = write_log(
        root,
        "logs/F0.1-git-status.log",
        (git_status.stdout or "") + "\n---STDERR---\n" + (git_status.stderr or ""),
    )
    record(
        root,
        phase="F0",
        batch="F0.1",
        scenario="git-status",
        kind="command",
        argv=["git", "status", "--short", "--branch"],
        cwd=str(REPO),
        started_at=started,
        ended_at=utc_now(),
        duration_ms=int((time.time() - t0) * 1000),
        exit_code=git_status.returncode,
        status="passed" if git_status.returncode == 0 else "failed",
        evidence_level="E0",
        log_rel=status_log["log"],
        sha256=status_log["sha256"],
        bytes_=status_log["bytes"],
        fingerprint_hex="pending",
        postconditions=[
            "tracked_modified=%d" % tracked_n,
            "nonignored_untracked=%d" % untracked_n,
            "ignored_listed=%d" % ignored_n,
            "identity_ok=%s" % identity_ok,
            "head=%s" % head,
            "branch=%s" % branch,
        ],
    )

    site = {
        "canonicalPath": canonical,
        "head": head,
        "branch": branch,
        "trackedModified": tracked_n,
        "nonignoredUntracked": untracked_n,
        "ignoredListed": ignored_n,
        "os": platform.platform(),
        "arch": platform.machine(),
        "python": sys.version,
        "disk": disk_free(),
        "writer": writer_snapshot(),
        "lineCounts": line_counts(),
        "runDirSize": dir_size_bounded(REPO / "tools" / "computer-use-probe" / ".run"),
        "appExe": {
            "path": str(APP_EXE),
            "exists": APP_EXE.is_file(),
            "bytes": APP_EXE.stat().st_size if APP_EXE.is_file() else None,
            "mtime": datetime.fromtimestamp(APP_EXE.stat().st_mtime).isoformat()
            if APP_EXE.is_file()
            else None,
        },
        "stubExe": {"path": str(STUB_EXE), "exists": STUB_EXE.is_file()},
        "oldD12D13D14": "invalidated_not_reused",
    }
    atomic_write_json(root / "baseline" / "site.json", site)
    site_log = write_log(
        root, "logs/F0.1-site.json", json.dumps(site, indent=2, ensure_ascii=False) + "\n"
    )
    record(
        root,
        phase="F0",
        batch="F0.1",
        scenario="site",
        kind="check",
        argv=["finalization_protocol", "site"],
        cwd=str(REPO),
        started_at=utc_now(),
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0,
        status="passed",
        evidence_level="E0",
        log_rel=site_log["log"],
        sha256=site_log["sha256"],
        bytes_=site_log["bytes"],
        fingerprint_hex="pending",
        postconditions=["site.json written", "old D12/D13/D14 not reused"],
    )

    fp_started = utc_now()
    t1 = time.time()
    fp = fingerprint()
    included = fp.pop("included")
    atomic_write_json(root / "baseline" / "fingerprint.json", fp)
    atomic_write_json(root / "baseline" / "untracked-source.json", included)
    fp_log = write_log(
        root,
        "logs/F0.2-fingerprint.json",
        json.dumps(
            {
                "codeFingerprint": fp["codeFingerprint"],
                "head": fp["head"],
                "branch": fp["branch"],
                "trackedDiffSha256": fp["trackedDiffSha256"],
                "untrackedMerkle": fp["untrackedMerkle"],
                "lockDigest": fp["lockDigest"],
                "includedCount": fp["includedCount"],
            },
            indent=2,
        )
        + "\n",
    )
    record(
        root,
        phase="F0",
        batch="F0.2",
        scenario="fingerprint",
        kind="command",
        argv=["finalization_protocol", "fingerprint"],
        cwd=str(REPO),
        started_at=fp_started,
        ended_at=utc_now(),
        duration_ms=int((time.time() - t1) * 1000),
        exit_code=0,
        status="passed",
        evidence_level="E0",
        log_rel=fp_log["log"],
        sha256=fp_log["sha256"],
        bytes_=fp_log["bytes"],
        fingerprint_hex=fp["codeFingerprint"],
        postconditions=["codeFingerprint=" + fp["codeFingerprint"]],
    )

    def set_fp(state):
        state["fingerprint"] = fp["codeFingerprint"]
        state["fingerprintDetails"] = {
            "head": fp["head"],
            "trackedDiffSha256": fp["trackedDiffSha256"],
            "untrackedMerkle": fp["untrackedMerkle"],
            "lockDigest": fp["lockDigest"],
        }

    update_state(root, set_fp)

    docs = [
        "AGENTS.md",
        "docs/llm-wiki/computer-use.md",
        "docs/plans/2026-09-09-computer-use-grok-step-plan.md",
        "docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md",
        "docs/plans/2026-09-11-computer-use-grok-post-overnight-audit.md",
        "docs/plans/2026-09-13-computer-use-grok-post-48h-audit.md",
        "docs/plans/2026-09-13-computer-use-grok-finalization-execution.md",
        "docs/plans/2026-09-09-computer-use-execution-state.md",
    ]
    doc_rows = []
    for rel in docs:
        p = REPO / rel
        doc_rows.append(
            {
                "path": rel,
                "exists": p.is_file(),
                "bytes": p.stat().st_size if p.is_file() else None,
            }
        )
    docs_log = write_log(
        root,
        "logs/F0.3-docs-read.json",
        json.dumps({"read": doc_rows}, indent=2) + "\n",
    )
    record(
        root,
        phase="F0",
        batch="F0.3",
        scenario="required-docs",
        kind="check",
        argv=["finalization_protocol", "docs-read"],
        cwd=str(REPO),
        started_at=utc_now(),
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0 if all(r["exists"] for r in doc_rows) else 1,
        status="passed" if all(r["exists"] for r in doc_rows) else "failed",
        evidence_level="E0",
        log_rel=docs_log["log"],
        sha256=docs_log["sha256"],
        bytes_=docs_log["bytes"],
        fingerprint_hex=fp["codeFingerprint"],
        postconditions=["required docs present"],
    )

    cp = {
        "F0-src": "src-tauri/computer-use-core/src/browser/worker_http.rs still uses reqwest::blocking::Client",
        "F0-callchain": "app_shell.rs revoke -> sessions::revoke -> request_stop -> ExistingTabHost::cancel_run -> bounded_loopback_post",
        "F0-old-reports": "D12/D13/D14 passed not reused; marked invalidated",
    }
    atomic_write_json(root / "baseline" / "conflicts.json", cp)
    print("F0 init root=%s fingerprint=%s" % (root, fp["codeFingerprint"]))
    return root


def f0_appshell_red() -> int:
    root = evidence_root()
    fp = load_json(root / "baseline" / "fingerprint.json")["codeFingerprint"]
    started = utc_now()
    t0 = time.time()
    home = root / "homes" / "f0-appshell-red"
    if home.exists():
        shutil.rmtree(home, ignore_errors=True)
    home.mkdir(parents=True)
    # Do not copy the old 48h runtime tree: Windows MAX_PATH breaks
    # playwright-core vite asset filenames. Isolated home +
    # GROK_CU_RESOURCE_DIR lets the harness repair from the seed.
    report = root / "logs" / "F0.4-harness-report.json"
    if report.exists():
        report.unlink()
    log_rel = "logs/F0.4-appshell-revoke-red.log"
    if not APP_EXE.is_file():
        body = "launcher unavailable: missing %s\n" % APP_EXE
        meta = write_log(root, log_rel, body)
        record(
            root,
            phase="F0",
            batch="F0.4",
            scenario="appshell-revoke-red",
            kind="command",
            argv=["grok-app.exe", "GROK_CU_APP_HARNESS=1"],
            cwd=str(REPO),
            started_at=started,
            ended_at=utc_now(),
            duration_ms=int((time.time() - t0) * 1000),
            exit_code=2,
            status="failed",
            evidence_level="E0",
            log_rel=meta["log"],
            sha256=meta["sha256"],
            bytes_=meta["bytes"],
            fingerprint_hex=fp,
            postconditions=["launcher_unavailable"],
        )
        (root / "failures" / "F0.4-launcher-unavailable.txt").write_text(body, encoding="utf-8")
        copy_to_scratch(root / meta["log"], SCRATCH_DEFAULT / "f0")
        return 2
    env = os.environ.copy()
    env.pop("GROK_CU_NODE_FILE", None)
    env["GROK_APP_INSTANCE_ID"] = "com.grokapp.desktop.cu-f0-red"
    env["GROK_APP_HOME"] = str(home)
    env["GROK_CU_APP_HARNESS"] = "1"
    env["GROK_CU_APP_HARNESS_OUT"] = str(report)
    env["GROK_CU_APP_HARNESS_ROUNDS"] = "1"
    env["GROK_CU_WEBVIEW_ROUNDS"] = "1"
    if STUB_EXE.is_file():
        env["GROK_CU_ACP_STUB"] = str(STUB_EXE)
    env["GROK_CU_RESOURCE_DIR"] = str(REPO / "src-tauri" / "resources")
    proc = subprocess.Popen(
        [str(APP_EXE)],
        cwd=str(REPO),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    timed_out = False
    try:
        out, _ = proc.communicate(timeout=240)
        code = proc.returncode
    except subprocess.TimeoutExpired:
        timed_out = True
        proc.kill()
        out, _ = proc.communicate()
        code = 124
    header = (
        "app=%s exit=%s timeout=%s ms=%d home=%s\n"
        % (APP_EXE, code, timed_out, int((time.time() - t0) * 1000), home)
    ).encode("utf-8")
    body = header + (out or b"")
    panic = b"Cannot drop a runtime in a context where blocking is not allowed" in body
    if report.is_file():
        body += b"\n--- harness ---\n" + report.read_bytes()
    meta = write_bytes(root, log_rel, body)
    copied_old = False
    if panic:
        status = "passed"
        post = [
            "raw_appshell_panic=true",
            "panic=Cannot drop a runtime in a context where blocking is not allowed",
            "not_copied_from_d13_d14=true",
        ]
    else:
        status = "failed"
        post = [
            "raw_appshell_panic=false",
            "exit=%s" % code,
            "need live equivalent panic or honest fail",
        ]
        (root / "failures" / "F0.4-no-panic-text.txt").write_text(
            "panic_marker_missing exit=%s timeout=%s\n" % (code, timed_out),
            encoding="utf-8",
        )
    record(
        root,
        phase="F0",
        batch="F0.4",
        scenario="appshell-revoke-red",
        kind="command",
        argv=["grok-app.exe", "GROK_CU_APP_HARNESS=1", "rounds=1"],
        cwd=str(REPO),
        started_at=started,
        ended_at=utc_now(),
        duration_ms=int((time.time() - t0) * 1000),
        exit_code=code if code is not None else 1,
        status=status,
        evidence_level="E3",
        log_rel=meta["log"],
        sha256=meta["sha256"],
        bytes_=meta["bytes"],
        fingerprint_hex=fp,
        postconditions=post,
    )
    scratch = SCRATCH_DEFAULT / "f0"
    scratch.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(root / log_rel), str(scratch / "F0.4-appshell-revoke-red.log"))
    if report.is_file():
        shutil.copy2(str(report), str(scratch / "F0.4-harness-report.json"))
    print(
        "F0 appshell-red panic=%s exit=%s log=%s copied_old=%s"
        % (panic, code, meta["log"], copied_old)
    )
    return 0 if panic else 1


def f1_appshell(rounds=20, webview_rounds=20) -> int:
    root = evidence_root()
    fp = load_json(root / "baseline" / "fingerprint.json")["codeFingerprint"]
    started = utc_now()
    t0 = time.time()
    seq = next_seq(root / "manifest.jsonl")
    home = root / "homes" / ("f1-appshell-%02d" % seq)
    if home.exists():
        shutil.rmtree(home, ignore_errors=True)
    home.mkdir(parents=True)
    report = root / "logs" / ("F1.appshell-%02d-report.json" % seq)
    log_rel = "logs/F1.appshell-%02d.log" % seq
    if not APP_EXE.is_file():
        body = "launcher unavailable: missing %s\n" % APP_EXE
        meta = write_log(root, log_rel, body)
        record(
            root,
            phase="F1",
            batch="F1.appshell",
            scenario="appshell-loops",
            kind="command",
            argv=["grok-app.exe", "GROK_CU_APP_HARNESS=1"],
            cwd=str(REPO),
            started_at=started,
            ended_at=utc_now(),
            duration_ms=int((time.time() - t0) * 1000),
            exit_code=2,
            status="failed",
            evidence_level="E0",
            log_rel=meta["log"],
            sha256=meta["sha256"],
            bytes_=meta["bytes"],
            fingerprint_hex=fp,
            postconditions=["launcher_unavailable"],
        )
        scratch = SCRATCH_DEFAULT / "f1-launcher-unavailable.log"
        scratch.write_text(body, encoding="utf-8")
        return 2
    env = os.environ.copy()
    env.pop("GROK_CU_NODE_FILE", None)
    env["GROK_APP_INSTANCE_ID"] = "com.grokapp.desktop.cu-f1-loop"
    env["GROK_APP_HOME"] = str(home)
    env["GROK_CU_APP_HARNESS"] = "1"
    env["GROK_CU_APP_HARNESS_OUT"] = str(report)
    env["GROK_CU_APP_HARNESS_ROUNDS"] = str(int(rounds))
    env["GROK_CU_WEBVIEW_ROUNDS"] = str(int(webview_rounds))
    if STUB_EXE.is_file():
        env["GROK_CU_ACP_STUB"] = str(STUB_EXE)
    env["GROK_CU_RESOURCE_DIR"] = str(REPO / "src-tauri" / "resources")
    proc = subprocess.Popen(
        [str(APP_EXE)],
        cwd=str(REPO),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    timed_out = False
    try:
        out, _ = proc.communicate(timeout=900)
        code = proc.returncode
    except subprocess.TimeoutExpired:
        timed_out = True
        proc.kill()
        out, _ = proc.communicate()
        code = 124
    header = (
        "app=%s exit=%s timeout=%s ms=%d home=%s rounds=%s wv=%s\n"
        % (
            APP_EXE,
            code,
            timed_out,
            int((time.time() - t0) * 1000),
            home,
            rounds,
            webview_rounds,
        )
    ).encode("utf-8")
    body = header + (out or b"")
    panic = b"Cannot drop a runtime in a context where blocking is not allowed" in body
    inner_ok = 0
    wv_ok = 0
    harness_ok = False
    if report.is_file():
        raw = report.read_bytes()
        body += b"\n--- harness ---\n" + raw
        data = json.loads(raw.decode("utf-8"))
        harness_ok = bool(data.get("ok"))
        inner_ok = int(data.get("innerRoundsOk") or 0)
        wv_ok = int(data.get("webviewRounds") or 0)
    meta = write_bytes(root, log_rel, body)
    passed = (
        (not panic)
        and (not timed_out)
        and code == 0
        and harness_ok
        and inner_ok >= int(rounds)
        and wv_ok >= int(webview_rounds)
    )
    record(
        root,
        phase="F1",
        batch="F1.appshell",
        scenario="appshell-loops",
        kind="command",
        argv=["grok-app.exe", "GROK_CU_APP_HARNESS=1", "rounds=%s" % rounds],
        cwd=str(REPO),
        started_at=started,
        ended_at=utc_now(),
        duration_ms=int((time.time() - t0) * 1000),
        exit_code=code if code is not None else 1,
        status="passed" if passed else "failed",
        evidence_level="E3",
        log_rel=meta["log"],
        sha256=meta["sha256"],
        bytes_=meta["bytes"],
        fingerprint_hex=fp,
        postconditions=[
            "panic=%s" % panic,
            "inner_ok=%s" % inner_ok,
            "webview_ok=%s" % wv_ok,
            "exit=%s" % code,
        ],
    )
    scratch = SCRATCH_DEFAULT / "f1-appshell"
    scratch.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(root / log_rel), str(scratch / Path(log_rel).name))
    if report.is_file():
        shutil.copy2(str(report), str(scratch / report.name))
    print(
        "F1 appshell passed=%s panic=%s exit=%s inner=%s wv=%s"
        % (passed, panic, code, inner_ok, wv_ok)
    )
    return 0 if passed else 1


def f0_checkpoint() -> None:
    root = evidence_root()
    fp = load_json(root / "baseline" / "fingerprint.json")["codeFingerprint"]

    def mut(state):
        f0 = state["batches"]["F0"]
        ignore_ok = True
        red_ok = False
        for line in (root / "manifest.jsonl").read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            if rec.get("batch") == "F0.4" and rec.get("status") == "passed":
                red_ok = True
        f0["status"] = "passed" if ignore_ok and red_ok else "in_progress"
        f0["highestEvidence"] = "E3" if red_ok else "E0"
        f0["sub"] = {
            "evidence_root": "passed" if ignore_ok else "failed",
            "appshell_red": "passed" if red_ok else "failed",
        }

    update_state(root, mut)
    ck = {
        "at": utc_now(),
        "phase": "F0",
        "fingerprint": fp,
        "state": load_json(root / "state.json")["batches"]["F0"],
    }
    atomic_write_json(root / "checkpoints" / "F0.json", ck)
    scratch = SCRATCH_DEFAULT / "f0"
    scratch.mkdir(parents=True, exist_ok=True)
    for name in ("owner.json", "state.json", "manifest.jsonl"):
        shutil.copy2(str(root / name), str(scratch / name))
    for name in ("fingerprint.json", "site.json", "untracked-source.json", "conflicts.json"):
        src = root / "baseline" / name
        if src.is_file():
            shutil.copy2(str(src), str(scratch / name))
    print("F0 checkpoint copied to %s" % scratch)


def f3f4_appshell(managed_rounds=20, webview_rounds=20) -> int:
    root = evidence_root()
    fp = load_json(root / "baseline" / "fingerprint.json")["codeFingerprint"]
    started = utc_now()
    t0 = time.time()
    seq = next_seq(root / "manifest.jsonl")
    home = root / "homes" / ("f3f4-appshell-%02d" % seq)
    if home.exists():
        shutil.rmtree(home, ignore_errors=True)
    home.mkdir(parents=True)
    report = root / "logs" / ("F3F4.appshell-%02d-report.json" % seq)
    log_rel = "logs/F3F4.appshell-%02d.log" % seq
    if not APP_EXE.is_file():
        body = "launcher unavailable: missing %s\n" % APP_EXE
        meta = write_log(root, log_rel, body)
        record(
            root,
            phase="F3",
            batch="F3F4.appshell",
            scenario="managed-webview-product",
            kind="command",
            argv=["grok-app.exe", "GROK_CU_APP_HARNESS=1", "managed=%s" % managed_rounds],
            cwd=str(REPO),
            started_at=started,
            ended_at=utc_now(),
            duration_ms=int((time.time() - t0) * 1000),
            exit_code=2,
            status="failed",
            evidence_level="E0",
            log_rel=meta["log"],
            sha256=meta["sha256"],
            bytes_=meta["bytes"],
            fingerprint_hex=fp,
            postconditions=["launcher_unavailable"],
        )
        return 2
    env = os.environ.copy()
    env.pop("GROK_CU_NODE_FILE", None)
    env["GROK_APP_INSTANCE_ID"] = "com.grokapp.desktop.cu-f3f4"
    env["GROK_APP_HOME"] = str(home)
    env["GROK_CU_APP_HARNESS"] = "1"
    env["GROK_CU_APP_HARNESS_OUT"] = str(report)
    env["GROK_CU_APP_HARNESS_ROUNDS"] = "2"
    env["GROK_CU_WEBVIEW_ROUNDS"] = str(int(webview_rounds))
    env["GROK_CU_MANAGED_ROUNDS"] = str(int(managed_rounds))
    if STUB_EXE.is_file():
        env["GROK_CU_ACP_STUB"] = str(STUB_EXE)
    env["GROK_CU_RESOURCE_DIR"] = str(REPO / "src-tauri" / "resources")
    proc = subprocess.Popen(
        [str(APP_EXE)],
        cwd=str(REPO),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    timed_out = False
    try:
        out, _ = proc.communicate(timeout=1800)
        code = proc.returncode
    except subprocess.TimeoutExpired:
        timed_out = True
        proc.kill()
        out, _ = proc.communicate()
        code = 124
    header = (
        "app=%s exit=%s timeout=%s ms=%d home=%s managed=%s wv=%s\n"
        % (
            APP_EXE,
            code,
            timed_out,
            int((time.time() - t0) * 1000),
            home,
            managed_rounds,
            webview_rounds,
        )
    ).encode("utf-8")
    body = header + (out or b"")
    panic = b"Cannot drop a runtime in a context where blocking is not allowed" in body
    managed_ok = 0
    wv_ok = 0
    harness_ok = False
    fail_reclaim = False
    if report.is_file():
        raw = report.read_bytes()
        body += b"\n--- harness ---\n" + raw
        data = json.loads(raw.decode("utf-8"))
        harness_ok = bool(data.get("ok"))
        managed_ok = int(data.get("managedRoundsOk") or 0)
        wv_ok = int(data.get("webviewRounds") or 0)
        for row in data.get("scenarios") or []:
            if row.get("name") == "webview_fail_reclaim" and row.get("ok"):
                fail_reclaim = True
    meta = write_bytes(root, log_rel, body)
    passed = (
        (not panic)
        and (not timed_out)
        and code == 0
        and harness_ok
        and managed_ok >= int(managed_rounds)
        and wv_ok >= int(webview_rounds)
        and fail_reclaim
    )
    record(
        root,
        phase="F3",
        batch="F3F4.appshell",
        scenario="managed-webview-product",
        kind="command",
        argv=["grok-app.exe", "GROK_CU_MANAGED_ROUNDS=%s" % managed_rounds],
        cwd=str(REPO),
        started_at=started,
        ended_at=utc_now(),
        duration_ms=int((time.time() - t0) * 1000),
        exit_code=code if code is not None else 1,
        status="passed" if passed else "failed",
        evidence_level="E3",
        log_rel=meta["log"],
        sha256=meta["sha256"],
        bytes_=meta["bytes"],
        fingerprint_hex=fp,
        postconditions=[
            "panic=%s" % panic,
            "managed_ok=%s" % managed_ok,
            "webview_ok=%s" % wv_ok,
            "fail_reclaim=%s" % fail_reclaim,
            "exit=%s" % code,
        ],
    )
    scratch = SCRATCH_DEFAULT / "f3f4"
    scratch.mkdir(parents=True, exist_ok=True)
    shutil.copy2(str(root / log_rel), str(scratch / Path(log_rel).name))
    if report.is_file():
        shutil.copy2(str(report), str(scratch / report.name))
    print(
        "F3F4 appshell passed=%s panic=%s exit=%s managed=%s wv=%s reclaim=%s"
        % (passed, panic, code, managed_ok, wv_ok, fail_reclaim)
    )
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "cmd",
        choices=[
            "f0-init",
            "f0-appshell-red",
            "f0-checkpoint",
            "f1-appshell",
            "f3f4-appshell",
            "fingerprint",
        ],
    )
    args = parser.parse_args()
    if args.cmd == "f0-init":
        f0_init()
        return 0
    if args.cmd == "f0-appshell-red":
        return f0_appshell_red()
    if args.cmd == "f0-checkpoint":
        f0_checkpoint()
        return 0
    if args.cmd == "f1-appshell":
        return f1_appshell()
    if args.cmd == "f3f4-appshell":
        return f3f4_appshell()
    if args.cmd == "fingerprint":
        fp = fingerprint()
        fp.pop("included", None)
        print(json.dumps({"codeFingerprint": fp["codeFingerprint"]}, indent=2))
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
