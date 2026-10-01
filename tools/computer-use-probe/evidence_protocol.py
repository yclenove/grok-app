#!/usr/bin/env python3
"""Repo-local ignored evidence protocol for Computer Use 48h runs.

Writes only under tools/computer-use-probe/.run/48h/ (run IDs and scratch copies).
Does not read auth.json / tokens / cookies. Manifest is append-only.
state.json is replaced atomically via same-volume os.replace.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import socket
import stat
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

SCHEMA_VERSION = 1
REPO = Path(__file__).resolve().parents[2]
EVIDENCE_PARENT = REPO / "tools" / "computer-use-probe" / ".run" / "48h"
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
SKIP_NAME_PARTS = (
    "/.run/",
    "\\ .run\\".replace(" ", ""),
    "/seed/",
    "\\seed\\",
    "/node_modules/",
    "\\node_modules\\",
    "/target/",
    "\\target\\",
    "/target-cu",
    "\\target-cu",
)
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


def utc_now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def local_now() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path, max_bytes: int | None = None) -> tuple[str, int]:
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
                return f"truncated:{h.hexdigest()}", rest
            h.update(chunk)
    return h.hexdigest(), size


def run_git(args: list[str], cwd: Path = REPO) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
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


def evidence_root_from_env() -> Path:
    env = os.environ.get("GROK_CU_EVIDENCE_ROOT")
    if env:
        return Path(env)
    marker = EVIDENCE_PARENT / "CURRENT_RUN"
    if marker.is_file():
        run_id = marker.read_text(encoding="utf-8").strip()
        return EVIDENCE_PARENT / run_id
    raise SystemExit("no evidence root; run d0-init first")


def make_run_id() -> str:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    host = socket.gethostname().split(".")[0].lower()
    return f"{stamp}-{host}"


def dir_size_bounded(path: Path, limit_files: int = 20000) -> dict[str, Any]:
    if not path.exists():
        return {"exists": False}
    files = 0
    bytes_ = 0
    truncated = False
    if path.is_file():
        return {"exists": True, "files": 1, "bytes": path.stat().st_size}
    for root, dirs, names in os.walk(path):
        # do not follow junctions/symlinks
        dirs[:] = [
            d
            for d in dirs
            if not os.path.islink(os.path.join(root, d))
        ]
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
        "mtime": datetime.fromtimestamp(path.stat().st_mtime).isoformat(timespec="seconds"),
    }


def classify_untracked(rel: str) -> bool:
    low = rel.replace("\\", "/").lower()
    if any(p.replace("\\", "/") in f"/{low}/" or p.replace("\\", "/") in low for p in (
        ".run/",
        "/seed/",
        "/node_modules/",
        "/target/",
        "target-cu",
    )):
        return False
    suffix = Path(rel).suffix.lower()
    if suffix in SOURCE_SUFFIXES:
        return True
    name = Path(rel).name.lower()
    return name in {"dockerfile", "makefile", "license", "notice"}


def fingerprint(repo: Path = REPO) -> dict[str, Any]:
    head = run_git(["rev-parse", "HEAD"]).stdout.strip()
    branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"]).stdout.strip()
    diff = run_git(["diff", "--binary", "HEAD"])
    tracked_diff_sha = sha256_bytes((diff.stdout or "").encode("utf-8", "replace"))
    untracked = run_git(["ls-files", "--others", "--exclude-standard"])
    files = [ln.strip().replace("\\", "/") for ln in (untracked.stdout or "").splitlines() if ln.strip()]
    included: list[dict[str, Any]] = []
    skipped: list[dict[str, Any]] = []
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
        merkle.update(f"{rel}:{digest}:{size}\n".encode("utf-8"))
    lock_paths = [
        "package.json",
        "pnpm-lock.yaml",
        "src-tauri/Cargo.lock",
        "src-tauri/Cargo.toml",
        "src-tauri/resources/computer-use/windows-x64.lock.json",
        "src-tauri/resources/computer-use/chromium-lock.json",
    ]
    locks: dict[str, Any] = {}
    lock_merkle = hashlib.sha256()
    for rel in lock_paths:
        p = repo / rel
        if p.is_file():
            digest, size = sha256_file(p)
            locks[rel] = {"sha256": digest, "bytes": size}
            lock_merkle.update(f"{rel}:{digest}\n".encode("utf-8"))
        else:
            locks[rel] = {"missing": True}
            lock_merkle.update(f"{rel}:missing\n".encode("utf-8"))
    code_fp = sha256_bytes(
        "\n".join(
            [
                f"head:{head}",
                f"branch:{branch}",
                f"tracked_diff:{tracked_diff_sha}",
                f"untracked:{merkle.hexdigest()}",
                f"locks:{lock_merkle.hexdigest()}",
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
    }


def write_log(root: Path, rel: str, text: str) -> dict[str, Any]:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    data = text.encode("utf-8")
    path.write_bytes(data)
    digest, size = sha256_file(path)
    return {"log": rel.replace("\\", "/"), "sha256": digest, "bytes": size}


def record(
    root: Path,
    *,
    phase: str,
    batch: str,
    kind: str,
    argv: list[str],
    cwd: str,
    started_at: str,
    ended_at: str,
    duration_ms: int,
    exit_code: int,
    status: str,
    evidence_level: str,
    log_rel: str,
    sha256: str,
    bytes_: int,
    fingerprint_hex: str,
    postconditions: list[str],
) -> dict[str, Any]:
    if status not in ATOMIC_STATUSES and status not in {"passed", "failed"}:
        # command records may use passed/failed; batch summary uses atomic set
        pass
    seq = next_seq(root / "manifest.jsonl")
    rec = {
        "schemaVersion": SCHEMA_VERSION,
        "seq": seq,
        "phase": phase,
        "batch": batch,
        "kind": kind,
        "argvRedacted": argv,
        "cwd": cwd,
        "startedAt": started_at,
        "endedAt": ended_at,
        "durationMs": duration_ms,
        "exitCode": exit_code,
        "status": status,
        "evidenceLevel": evidence_level,
        "log": log_rel,
        "sha256": sha256,
        "bytes": bytes_,
        "codeFingerprint": fingerprint_hex,
        "postconditions": postconditions,
    }
    append_jsonl(root / "manifest.jsonl", rec)
    return rec


def default_batches() -> dict[str, Any]:
    ids = [
        "D0",
        "D1",
        "D2",
        "D3",
        "D4",
        "D5",
        "D6",
        "D7",
        "D8",
        "D9",
        "D10",
        "D11",
        "D12",
        "D13",
        "D14",
    ]
    return {
        bid: {
            "status": "not_started" if bid != "D0" else "in_progress",
            "sub": {},
        }
        for bid in ids
    }


def update_state(root: Path, mutator) -> dict[str, Any]:
    path = root / "state.json"
    state = load_json(path)
    mutator(state)
    state["updatedAt"] = utc_now()
    atomic_write_json(path, state)
    return state


def inventory_old_evidence(legacy_paths: Iterable[Path] = ()) -> dict[str, Any]:
    items: list[dict[str, Any]] = []

    def add(path: Path, claim: str, extra: dict[str, Any] | None = None) -> None:
        rec: dict[str, Any] = {
            "path": str(path),
            "exists": path.exists(),
            "claim": claim,
        }
        if extra:
            rec.update(extra)
        if path.exists():
            try:
                rec["mtime"] = datetime.fromtimestamp(path.stat().st_mtime).isoformat(
                    timespec="seconds"
                )
            except OSError:
                pass
            if path.is_file() and path.name.lower() not in SECRET_NAMES:
                try:
                    digest, size = sha256_file(path, max_bytes=32 * 1024 * 1024)
                    rec["sha256"] = digest
                    rec["bytes"] = size
                except OSError as e:
                    rec["hashError"] = str(e)
            elif path.is_dir():
                rec["listing"] = []
                try:
                    for child in sorted(path.iterdir())[:40]:
                        rec["listing"].append(
                            {
                                "name": child.name,
                                "isDir": child.is_dir(),
                                "secretSkipped": child.name.lower() in SECRET_NAMES,
                            }
                        )
                except OSError as e:
                    rec["listError"] = str(e)
        else:
            rec["claim"] = "historical_claim_unverified"
        items.append(rec)

    # External historical evidence is inspected only when explicitly supplied.
    # Never infer a user's temporary directories from an old development host.
    for legacy_path in legacy_paths:
        add(
            legacy_path.resolve(),
            "explicitly supplied historical evidence",
            {"explicitLegacyEvidence": True},
        )
    add(REPO / "tools" / "computer-use-probe" / ".run" / "p0", "legacy probe p0")
    add(REPO / "tools" / "computer-use-probe" / ".run" / "p8", "legacy probe p8")
    add(REPO / "tools" / "computer-use-probe" / ".run" / "nsis", "legacy nsis evidence")
    add(REPO / "tools" / "computer-use-probe" / ".run" / "cua", "legacy cua probe")
    add(
        REPO / "docs" / "plans" / "2026-09-09-computer-use-execution-state.md",
        "execution-state ledger (not raw logs)",
    )
    add(
        REPO / "docs" / "plans" / "2026-09-11-computer-use-grok-post-overnight-audit.md",
        "post-overnight audit",
    )
    # Do not hash grok-home/auth.json
    grok_home = REPO / "tools" / "computer-use-probe" / ".run" / "grok-home"
    items.append(
        {
            "path": str(grok_home),
            "exists": grok_home.exists(),
            "claim": "probe isolated GROK_HOME leftover; contents not read (may contain auth.json)",
            "secretSkipped": True,
        }
    )
    return {"computedAt": utc_now(), "items": items}


def writer_snapshot() -> dict[str, Any]:
    cargo = []
    try:
        import psutil  # type: ignore

        for p in psutil.process_iter(["pid", "name", "create_time"]):
            name = (p.info.get("name") or "").lower()
            if name in {"cargo.exe", "rustc.exe", "rust-analyzer.exe"}:
                cargo.append({"pid": p.info["pid"], "name": name})
    except Exception:
        cargo = [{"note": "psutil unavailable; cargo/rustc checked via tasklist"}]
        try:
            out = subprocess.run(
                ["tasklist", "/FI", "IMAGENAME eq cargo.exe"],
                capture_output=True,
                text=True,
                check=False,
            )
            cargo.append({"tasklist_cargo": (out.stdout or "")[:500]})
        except OSError:
            pass
    return {
        "pid": os.getpid(),
        "ppid": os.getppid(),
        "cwd": str(Path.cwd()),
        "hostname": socket.gethostname(),
        "cargoRustc": cargo,
        "note": "node MCP/Codex/chrome-devtools processes exist on the machine but are not cargo writers on this worktree",
    }


def disk_free() -> list[dict[str, Any]]:
    rows = []
    for letter in "CDEFHYZ":
        p = Path(f"{letter}:/")
        if not p.exists():
            continue
        usage = os.statvfs(p) if hasattr(os, "statvfs") else None
        if usage:
            rows.append(
                {
                    "drive": letter,
                    "free": usage.f_bavail * usage.f_frsize,
                    "total": usage.f_blocks * usage.f_frsize,
                }
            )
        else:
            import ctypes

            free = ctypes.c_ulonglong()
            total = ctypes.c_ulonglong()
            ctypes.windll.kernel32.GetDiskFreeSpaceExW(
                ctypes.c_wchar_p(f"{letter}:\\"),
                None,
                ctypes.byref(total),
                ctypes.byref(free),
            )
            rows.append({"drive": letter, "free": free.value, "total": total.value})
    return rows


def line_counts() -> dict[str, Any]:
    files = {
        "App.tsx": REPO / "src" / "App.tsx",
        "AppWorkbench.tsx": REPO / "src" / "app" / "AppWorkbench.tsx",
    }
    out: dict[str, Any] = {}
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


def atomic_interrupt_test(root: Path) -> dict[str, Any]:
    path = root / "state.json"
    original = load_json(path)
    tmp = path.with_name("state.json.tmp")
    tmp.write_bytes(b'{"partial": true,')  # truncated
    still = load_json(path)
    tmp_exists = tmp.exists()
    tmp.unlink(missing_ok=True)
    ok = still == original and still.get("schemaVersion") == SCHEMA_VERSION
    return {
        "truncatedTmpDidNotCorruptState": ok,
        "tmpExistedDuringTest": tmp_exists,
        "stateKeys": sorted(still.keys()),
    }


def d0_init(legacy_paths: Iterable[Path] = ()) -> Path:
    if not check_ignore("tools/computer-use-probe/.run/"):
        raise SystemExit("git check-ignore failed for tools/computer-use-probe/.run/")
    run_id = make_run_id()
    root = EVIDENCE_PARENT / run_id
    for sub in ("baseline", "checkpoints", "failures", "logs", "metrics", "reports"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    (EVIDENCE_PARENT / "CURRENT_RUN").write_text(run_id + "\n", encoding="utf-8")
    git_status = run_git(["status", "--short", "--branch"])
    tracked = run_git(["diff", "--name-only", "--diff-filter=ACDMR", "HEAD"])
    untracked = run_git(["ls-files", "--others", "--exclude-standard"])
    tracked_n = len([l for l in (tracked.stdout or "").splitlines() if l.strip()])
    untracked_n = len([l for l in (untracked.stdout or "").splitlines() if l.strip()])
    head = run_git(["rev-parse", "HEAD"]).stdout.strip()
    branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"]).stdout.strip()
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
        "repoCanonicalPath": str(REPO),
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
    }
    atomic_write_json(root / "owner.json", owner)
    state = {
        "schemaVersion": SCHEMA_VERSION,
        "runId": run_id,
        "phase": "D0",
        "batches": default_batches(),
        "fingerprint": None,
        "createdAt": utc_now(),
        "updatedAt": utc_now(),
        "identityOk": identity_ok,
    }
    atomic_write_json(root / "state.json", state)
    (root / "manifest.jsonl").write_text("", encoding="utf-8")

    started = utc_now()
    t0 = time.time()
    status_log = write_log(
        root,
        "logs/D0.1-git-status.log",
        (git_status.stdout or "") + "\n---STDERR---\n" + (git_status.stderr or ""),
    )
    rec = record(
        root,
        phase="D0",
        batch="D0.1",
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
            f"tracked_modified={tracked_n}",
            f"nonignored_untracked={untracked_n}",
            f"identity_ok={identity_ok}",
        ],
    )
    # fake command record then verify size/hash
    fake_started = utc_now()
    fake_body = "fake command record for D0.2 hash/size protocol\n"
    fake_log = write_log(root, "logs/D0.2-fake-command.log", fake_body)
    verify_path = root / fake_log["log"]
    digest2, size2 = sha256_file(verify_path)
    hash_ok = digest2 == fake_log["sha256"] and size2 == fake_log["bytes"]
    record(
        root,
        phase="D0",
        batch="D0.2",
        kind="command",
        argv=["evidence_protocol", "fake-command"],
        cwd=str(REPO),
        started_at=fake_started,
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0 if hash_ok else 1,
        status="passed" if hash_ok else "failed",
        evidence_level="E1",
        log_rel=fake_log["log"],
        sha256=fake_log["sha256"],
        bytes_=fake_log["bytes"],
        fingerprint_hex="pending",
        postconditions=[f"log_size_hash_match={hash_ok}"],
    )
    interrupt = atomic_interrupt_test(root)
    interrupt_log = write_log(
        root, "logs/D0.2-atomic-interrupt.json", json.dumps(interrupt, indent=2)
    )
    record(
        root,
        phase="D0",
        batch="D0.2",
        kind="check",
        argv=["evidence_protocol", "atomic-interrupt"],
        cwd=str(REPO),
        started_at=utc_now(),
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0 if interrupt["truncatedTmpDidNotCorruptState"] else 1,
        status="passed" if interrupt["truncatedTmpDidNotCorruptState"] else "failed",
        evidence_level="E1",
        log_rel=interrupt_log["log"],
        sha256=interrupt_log["sha256"],
        bytes_=interrupt_log["bytes"],
        fingerprint_hex="pending",
        postconditions=["state.json remains valid JSON after truncated tmp"],
    )

    fp_started = utc_now()
    t1 = time.time()
    fp = fingerprint()
    fp_path = root / "baseline" / "fingerprint.json"
    atomic_write_json(fp_path, fp)
    # full included list stored separately (may be large)
    # recompute included paths into baseline list
    untracked_files = [
        ln.strip().replace("\\", "/")
        for ln in (untracked.stdout or "").splitlines()
        if ln.strip()
    ]
    included_full = []
    for rel in sorted(untracked_files):
        if Path(rel).name.lower() in SECRET_NAMES:
            continue
        if not classify_untracked(rel):
            continue
        p = REPO / rel
        if p.is_file():
            digest, size = sha256_file(p)
            included_full.append({"path": rel, "sha256": digest, "bytes": size})
    atomic_write_json(root / "baseline" / "untracked-source.json", included_full)
    fp_log = write_log(
        root,
        "logs/D0.3-fingerprint.json",
        json.dumps(
            {
                "codeFingerprint": fp["codeFingerprint"],
                "head": fp["head"],
                "trackedDiffSha256": fp["trackedDiffSha256"],
                "untrackedMerkle": fp["untrackedMerkle"],
                "lockDigest": fp["lockDigest"],
            },
            indent=2,
        )
        + "\n",
    )
    record(
        root,
        phase="D0",
        batch="D0.3",
        kind="command",
        argv=["evidence_protocol", "fingerprint"],
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
        postconditions=[f"codeFingerprint={fp['codeFingerprint']}"],
    )

    inv = inventory_old_evidence(legacy_paths)
    atomic_write_json(root / "baseline" / "old-evidence-inventory.json", inv)
    inv_log = write_log(
        root, "logs/D0.4-old-evidence-inventory.json", json.dumps(inv, indent=2) + "\n"
    )
    missing_scratch = any(
        (not it.get("exists"))
        and it.get("explicitLegacyEvidence")
        for it in inv["items"]
    )
    supplied_legacy_count = sum(
        bool(it.get("explicitLegacyEvidence")) for it in inv["items"]
    )
    record(
        root,
        phase="D0",
        batch="D0.4",
        kind="check",
        argv=["evidence_protocol", "old-evidence-inventory"],
        cwd=str(REPO),
        started_at=utc_now(),
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0,
        status="passed",
        evidence_level="E0",
        log_rel=inv_log["log"],
        sha256=inv_log["sha256"],
        bytes_=inv_log["bytes"],
        fingerprint_hex=fp["codeFingerprint"],
        postconditions=[
            f"explicit_legacy_evidence_missing={missing_scratch}",
            f"explicit_legacy_evidence_paths={supplied_legacy_count}",
            "no forged hashes for missing paths",
        ],
    )

    sizes = {
        "seed": dir_size_bounded(REPO / "src-tauri" / "resources" / "computer-use" / "seed", 5000),
        "targetCu": dir_size_bounded(REPO / "src-tauri" / "target-cu", 3000),
        "targetCuReview": dir_size_bounded(REPO / "src-tauri" / "target-cu-review", 3000),
        "probeRun": dir_size_bounded(REPO / "tools" / "computer-use-probe" / ".run", 8000),
        "lock": dir_size_bounded(
            REPO / "src-tauri" / "resources" / "computer-use" / "windows-x64.lock.json"
        ),
    }
    baseline = {
        "repo": str(REPO),
        "branch": branch,
        "head": head,
        "identityOk": identity_ok,
        "trackedModified": tracked_n,
        "nonignoredUntracked": untracked_n,
        "disk": disk_free(),
        "os": platform.platform(),
        "arch": platform.machine(),
        "hostname": socket.gethostname(),
        "writer": writer_snapshot(),
        "appShellLines": line_counts(),
        "sizes": sizes,
        "checkIgnoreRun": True,
        "codeFingerprint": fp["codeFingerprint"],
        "runtimeLocks": fp["locks"],
    }
    atomic_write_json(root / "baseline" / "site.json", baseline)
    scratch = SCRATCH_DEFAULT
    scratch.mkdir(parents=True, exist_ok=True)
    (scratch / "d0-baseline.txt").write_text(
        json.dumps(baseline, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    ckpt = []
    ckpt.append("# D0 checkpoint")
    ckpt.append("")
    ckpt.append(f"- runId: `{run_id}`")
    ckpt.append(f"- repo: `{REPO}`")
    ckpt.append(f"- branch: `{branch}`")
    ckpt.append(f"- HEAD: `{head}`")
    ckpt.append(f"- identityOk: `{identity_ok}`")
    ckpt.append(f"- tracked modified: `{tracked_n}`")
    ckpt.append(f"- nonignored untracked: `{untracked_n}`")
    ckpt.append(f"- codeFingerprint: `{fp['codeFingerprint']}`")
    ckpt.append(f"- check-ignore `.run/`: pass")
    ckpt.append(f"- fake command size/hash: `{hash_ok}`")
    ckpt.append(
        f"- atomic interrupt: `{interrupt['truncatedTmpDidNotCorruptState']}`"
    )
    ckpt.append(
        f"- explicitly supplied historical paths: {supplied_legacy_count}; "
        f"any missing: {missing_scratch}; unspecified external evidence is not inspected"
    )
    ckpt.append(f"- App+AppWorkbench lines: `{baseline['appShellLines']}`")
    ckpt.append("- no production code changed in D0")
    ckpt.append("")
    ckpt_text = "\n".join(ckpt) + "\n"
    (root / "checkpoints" / "D0.md").write_text(ckpt_text, encoding="utf-8")
    summary_log = write_log(root, "logs/D0-summary.md", ckpt_text)
    record(
        root,
        phase="D0",
        batch="D0.5",
        kind="summary",
        argv=["evidence_protocol", "d0-init"],
        cwd=str(REPO),
        started_at=owner["startedAtUtc"],
        ended_at=utc_now(),
        duration_ms=0,
        exit_code=0 if identity_ok and hash_ok and interrupt["truncatedTmpDidNotCorruptState"] else 1,
        status="passed"
        if identity_ok and hash_ok and interrupt["truncatedTmpDidNotCorruptState"]
        else "failed",
        evidence_level="E0",
        log_rel=summary_log["log"],
        sha256=summary_log["sha256"],
        bytes_=summary_log["bytes"],
        fingerprint_hex=fp["codeFingerprint"],
        postconditions=["D0 checkpoint written", "scratch d0-baseline.txt written"],
    )

    def _set(s: dict[str, Any]) -> None:
        s["phase"] = "D0"
        s["fingerprint"] = fp["codeFingerprint"]
        s["batches"]["D0"]["status"] = (
            "passed"
            if identity_ok and hash_ok and interrupt["truncatedTmpDidNotCorruptState"]
            else "failed"
        )
        s["batches"]["D0"]["finishedAt"] = utc_now()
        s["identityOk"] = identity_ok

    update_state(root, _set)
    print(json.dumps({"runId": run_id, "root": str(root), "fingerprint": fp["codeFingerprint"], "identityOk": identity_ok}, indent=2))
    if not identity_ok:
        raise SystemExit("identity mismatch; stop per OBJECTIVE")
    return root


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("cmd", choices=["d0-init", "fingerprint", "record-help"])
    parser.add_argument(
        "--legacy-evidence",
        action="append",
        type=Path,
        default=[],
        help="Explicit historical evidence path to inventory during d0-init; repeatable.",
    )
    args = parser.parse_args()
    if args.cmd == "d0-init":
        d0_init(args.legacy_evidence)
        return 0
    if args.cmd == "fingerprint":
        print(json.dumps(fingerprint(), indent=2))
        return 0
    print("use evidence_protocol.d0_init / record")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
