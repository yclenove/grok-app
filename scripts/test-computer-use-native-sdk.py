#!/usr/bin/env python3
"""Deterministic audit failures; only the explicit native smoke proves loading."""
import importlib.util
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("native_sdk", ROOT / "audit-computer-use-native-sdk.py")
sdk = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sdk)
REAL_READELF = sdk.readelf

HEADER = "Class: ELF64\nData: 2's complement, little endian\nType: DYN (Shared object file)\nMachine: Advanced Micro Devices X86-64\n"
DYNAMIC = "(NEEDED) Shared library: [libc.so.6]\n(SONAME) Library soname: [libpipewire-0.3.so.0]\n"
VERSIONS = "Name: GLIBC_2.2.5\nName: GLIBC_2.34\n"


class NativeSdkAudit(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve() / "runtime"
        self.root.mkdir()
        for relative in sdk.REQUIRED:
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"\x7fELF-inert-audit-fixture" if relative in sdk.LIBRARIES else b"inert notice fixture")
        (self.root / "share/licenses/pipewire/SOURCE.txt").write_text("\n".join(sdk.SOURCE_LINES) + "\n")
        # These are contract fixtures, not licensed upstream files or native DSOs.
        hashes = {relative: sdk.digest(self.root / relative) for relative in sdk.NOTICE_HASHES}
        self.addCleanup(patch.stopall)
        patch.object(sdk, "NOTICE_HASHES", hashes).start()
        self.reader = patch.object(sdk, "readelf", side_effect=self.readelf).start()

    def readelf(self, path, option):
        return {"--file-header": HEADER, "--dynamic": DYNAMIC, "--version-info": VERSIONS}[option]

    def test_complete_inventory_has_narrow_scope_and_hashes(self):
        report = sdk.audit(self.root)
        self.assertEqual(len(report["files"]), len(sdk.REQUIRED))
        self.assertFalse(report["installedAppAcceptance"])
        self.assertFalse(report["portalAcceptance"])
        self.assertFalse(report["captureInputAcceptance"])
        for item in report["files"]:
            self.assertEqual(item["sha256"], sdk.digest(self.root / item["path"]))

    def test_missing_each_component_or_notice_is_failure(self):
        for relative in sdk.REQUIRED:
            with self.subTest(relative=relative):
                path = self.root / relative
                saved = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(sdk.AuditError, "Missing runtime"):
                    sdk.audit(self.root)
                path.write_bytes(saved)

    def test_empty_file_is_not_presence_proof(self):
        (self.root / sdk.LIBRARIES[0]).write_bytes(b"")
        with self.assertRaisesRegex(sdk.AuditError, "Empty runtime file"):
            sdk.audit(self.root)

    def test_corrupt_library_is_not_elf(self):
        (self.root / sdk.LIBRARIES[0]).write_bytes(b"corrupt")
        with self.assertRaisesRegex(sdk.AuditError, "Not an ELF"):
            sdk.audit(self.root)

    def test_higher_glibc_is_rejected(self):
        self.reader.side_effect = lambda path, option: "Name: GLIBC_2.36\n" if option == "--version-info" else self.readelf(path, option)
        with self.assertRaisesRegex(sdk.AuditError, "exceeds baseline"):
            sdk.audit(self.root)

    def test_private_or_unknown_glibc_requirement_is_rejected(self):
        self.reader.side_effect = lambda path, option: "Name: GLIBC_PRIVATE\n" if option == "--version-info" else self.readelf(path, option)
        with self.assertRaisesRegex(sdk.AuditError, "Invalid numeric ABI"):
            sdk.audit(self.root)

    def test_libei_or_unapproved_dependency_is_rejected(self):
        for library in ("libei.so.1", "libeis.so.1", "libjackserver.so.0", "/tmp/host.so"):
            with self.subTest(library=library):
                self.reader.side_effect = lambda path, option: f"(NEEDED) Shared library: [{library}]\n" if option == "--dynamic" else self.readelf(path, option)
                with self.assertRaisesRegex(sdk.AuditError, "Unapproved dynamic dependency"):
                    sdk.audit(self.root)

    def test_build_runpath_and_wrong_soname_are_rejected(self):
        for dynamic in (DYNAMIC + "(RUNPATH) Library runpath: [/work/build]\n", DYNAMIC.replace("libpipewire-0.3.so.0", "host.so")):
            with self.subTest(dynamic=dynamic):
                self.reader.side_effect = lambda path, option: dynamic if option == "--dynamic" else self.readelf(path, option)
                with self.assertRaises(sdk.AuditError):
                    sdk.audit(self.root)

    def test_wrong_architecture_is_rejected(self):
        self.reader.side_effect = lambda path, option: HEADER.replace("Advanced Micro Devices X86-64", "AArch64") if option == "--file-header" else self.readelf(path, option)
        with self.assertRaisesRegex(sdk.AuditError, "Linux x64"):
            sdk.audit(self.root)

    def test_arm64_inventory_requires_arm64_elf_and_its_own_loader(self):
        target = "aarch64-unknown-linux-gnu"
        def arm_reader(path, option):
            if option == "--file-header":
                return HEADER.replace("Advanced Micro Devices X86-64", "AArch64")
            if option == "--dynamic":
                return DYNAMIC + "(NEEDED) Shared library: [ld-linux-aarch64.so.1]\n"
            return "Name: GLIBC_2.39\n"
        self.reader.side_effect = arm_reader
        report = sdk.audit(self.root, target=target)
        self.assertEqual(report["target"], target)
        self.assertEqual(report["glibcBaseline"], "2.39")
        self.assertEqual(report["scope"], "private-linux-arm64-client-sdk")
        with self.assertRaisesRegex(sdk.AuditError, "Linux x64"):
            sdk.audit(self.root)
        for bad_header, bad_dynamic, bad_versions, error in (
            (HEADER, arm_reader(None, "--dynamic"), "Name: GLIBC_2.39\n", "Linux arm64"),
            (arm_reader(None, "--file-header"), DYNAMIC + "(NEEDED) Shared library: [ld-linux-x86-64.so.2]\n", "Name: GLIBC_2.39\n", "Unapproved dynamic"),
            (arm_reader(None, "--file-header"), DYNAMIC, "Name: GLIBC_2.40\n", "exceeds baseline"),
        ):
            with self.subTest(error=error):
                self.reader.side_effect = lambda path, option: {
                    "--file-header": bad_header, "--dynamic": bad_dynamic,
                    "--version-info": bad_versions,
                }[option]
                with self.assertRaisesRegex(sdk.AuditError, error):
                    sdk.audit(self.root, target=target)

    def test_target_or_baseline_cannot_be_changed_to_accept_incompatible_libraries(self):
        for target, maximum, error in (
            (sdk.DEFAULT_TARGET, "2.39", "exceeds.*policy"),
            ("aarch64-unknown-linux-gnu", "2.40", "exceeds.*policy"),
            ("riscv64-unknown-linux-gnu", None, "Unsupported"),
        ):
            with self.subTest(target=target):
                with self.assertRaisesRegex(sdk.AuditError, error):
                    sdk.audit(self.root, maximum, target)
        self.reader.assert_not_called()

    def test_modified_license_patch_or_provenance_is_rejected(self):
        for relative in (*sdk.NOTICE_HASHES, "share/licenses/pipewire/SOURCE.txt"):
            with self.subTest(relative=relative):
                path = self.root / relative
                saved = path.read_bytes()
                path.write_bytes(saved + b"changed")
                with self.assertRaises(sdk.AuditError):
                    sdk.audit(self.root)
                path.write_bytes(saved)

    def test_extra_modules_are_rejected(self):
        (self.root / "lib/libjackserver.so").write_bytes(b"unapproved")
        with self.assertRaisesRegex(sdk.AuditError, "Unapproved runtime files"):
            sdk.audit(self.root)

    def test_empty_config_dropin_directory_is_rejected(self):
        (self.root / "share/pipewire/client.conf.d").mkdir()
        with self.assertRaisesRegex(sdk.AuditError, "Unapproved runtime directory"):
            sdk.audit(self.root)

    @unittest.skipUnless(sys.platform == "linux", "native symlink semantics")
    def test_links_and_linked_parents_are_rejected(self):
        path = self.root / sdk.LIBRARIES[0]
        path.unlink()
        path.symlink_to(Path(self.temp.name) / "outside")
        with self.assertRaisesRegex(sdk.AuditError, "Symlink or special"):
            sdk.audit(self.root)
        alias = Path(self.temp.name) / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(sdk.AuditError, "canonical"):
            sdk.audit(alias)

    @unittest.skipUnless(sys.platform == "linux", "native FIFO semantics")
    def test_special_file_is_rejected_without_reading(self):
        import os
        os.mkfifo(self.root / "fifo")
        with self.assertRaisesRegex(sdk.AuditError, "special file"):
            sdk.audit(self.root)

    def test_existing_inventory_is_not_overwritten(self):
        output = Path(self.temp.name).resolve() / "inventory.json"
        output.write_text("keep original")
        with patch.object(sys, "argv", ["audit", str(self.root), "--output", str(output)]):
            self.assertEqual(sdk.main(), 2)
        self.assertEqual(output.read_text(), "keep original")

    @unittest.skipUnless(sys.platform == "linux" and shutil.which("cc") and shutil.which("readelf"), "requires Linux ELF compiler/tools")
    def test_actual_compiled_elf_and_truncated_elf(self):
        with patch.object(sdk, "readelf", REAL_READELF):
            directory = Path(self.temp.name)
            source = directory / "fixture.c"
            library = directory / "fixture.so"
            source.write_text("#include <stdio.h>\nvoid fixture(void) { puts(\"inert\"); }\n")
            subprocess.run(["cc", "-shared", "-fPIC", str(source), "-o", str(library)], check=True)
            target = {"x86_64": sdk.DEFAULT_TARGET, "aarch64": "aarch64-unknown-linux-gnu"}[platform.machine()]
            self.assertIn("libc.so.6", sdk.inspect_elf(library, "2.99", target)["needed"])
            with self.assertRaisesRegex(sdk.AuditError, "exceeds baseline"):
                sdk.inspect_elf(library, "0.1", target)
            library.write_bytes(b"\x7fELFtruncated")
            with self.assertRaisesRegex(sdk.AuditError, "Malformed ELF"):
                sdk.inspect_elf(library, "2.35", target)

if __name__ == "__main__":
    unittest.main(verbosity=2)
