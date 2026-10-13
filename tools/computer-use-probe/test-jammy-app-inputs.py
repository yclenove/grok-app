"""Synthetic file-inventory oracles; not App/native execution acceptance."""
import hashlib
import json
import subprocess
import sys
import uuid
from pathlib import Path


def record(path):
    return f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path}\n"


def main():
    helper = Path(__file__).with_name("check-jammy-app-inputs.sh").resolve(strict=True)
    # The verifier deliberately permits only alphanumeric/hyphen owner names.
    # tempfile's random suffix can contain underscores, invalidating the fixture
    # before any content assertion runs. Keep the production boundary unchanged.
    root = Path("/var/tmp") / f"grok-cu-jammy-sdk-input-oracle-{uuid.uuid4().hex}"
    root.mkdir(mode=0o700)
    results = []
    try:
        stage = root / "work/product-app-fixture"
        source = stage / "src-tauri/source.rs"
        sidecar = stage / "src-tauri/resources/computer-use/.cu-mutation.lock.owner.json"
        seed = root / "work/product-app-original/seed/component"
        evidence = root / "evidence"
        for path, data in [(source, b"source\n"), (sidecar, b"owner-before\n"), (seed, b"immutable\n")]:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        evidence.mkdir()
        (evidence / "app-inputs.sha256").write_text(record(source) + record(sidecar), encoding="utf-8")
        (evidence / "original-seed.sha256").write_text(record(seed), encoding="utf-8")

        def check(name, expected):
            result = subprocess.run(["bash", str(helper), str(root), str(evidence), "/work/product-app-fixture"],
                                    capture_output=True, text=True, timeout=10)
            assert result.returncode == expected, f"{name}: {result.returncode} {result.stdout} {result.stderr}"
            results.append({"case": name, "exit": result.returncode, "passed": True})

        check("unchanged-inputs", 0)
        sidecar.write_bytes(b"owner-after\n")
        check("exact-generated-owner-change", 0)
        source.write_bytes(b"changed-source\n")
        check("source-change-rejected", 1)
        source.write_bytes(b"source\n")
        extra = stage / "unexpected-file"
        extra.write_bytes(b"not-in-snapshot\n")
        check("extra-file-rejected", 1)
        extra.unlink()
        seed.write_bytes(b"changed-seed\n")
        check("original-seed-change-rejected", 1)
        seed.write_bytes(b"immutable\n")
        source.unlink()
        check("missing-source-rejected", 1)
        print(json.dumps({"scope": "synthetic input verifier", "cases": results}, indent=2))
    finally:
        # Original subprocesses have joined. Remove only this exact generated
        # fixture, rejecting links, with file unlinks and empty-directory rmdir.
        assert root.parent == Path("/var/tmp") and root.name.startswith("grok-cu-jammy-sdk-input-oracle-")
        entries = list(root.rglob("*"))
        assert not root.is_symlink() and not any(path.is_symlink() for path in entries)
        logical_bytes = sum(path.stat().st_size for path in entries if path.is_file())
        for path in entries:
            if path.is_file():
                path.unlink()
        for path in sorted((path for path in entries if path.is_dir()), key=lambda p: len(p.parts), reverse=True):
            path.rmdir()
        root.rmdir()
        print(json.dumps({"removedFixtureRoot": str(root), "logicalBytes": logical_bytes,
                          "actualReclaimedBytes": None,
                          "regenerate": "python3 tools/computer-use-probe/test-jammy-app-inputs.py"}), file=sys.stderr)


if __name__ == "__main__":
    main()
