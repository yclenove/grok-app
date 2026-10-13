#!/usr/bin/env python3
"""Private SDK builder contracts, not native or installed acceptance."""
from pathlib import Path
import os
import subprocess
import shutil
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
BUILDER = ROOT / "build-computer-use-pipewire.sh"
CHECKER = ROOT / "check-computer-use-pipewire-runtime.sh"
BASH = shutil.which("bash")


@unittest.skipUnless(sys.platform == "linux", "requires Linux shell tooling")
class PipeWireSdkContract(unittest.TestCase):
    def invoke(self, arguments, env=None):
        return subprocess.run([BASH, str(BUILDER), *arguments], env=env,
                              capture_output=True, text=True, timeout=15)

    def test_requires_new_absolute_directory(self):
        for arguments in ([], ["relative"], ["/"], ["/a", "/b"]):
            with self.subTest(arguments=arguments):
                self.assertEqual(self.invoke(arguments).returncode, 2)

    def test_existing_directory_unchanged(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            marker = root / "keep.txt"
            marker.write_text("existing SDK must not be overwritten")
            self.assertEqual(self.invoke([temp]).returncode, 2)
            self.assertEqual(marker.read_text(), "existing SDK must not be overwritten")
            self.assertEqual(list(root.iterdir()), [marker])

    def test_symlink_and_dangling_symlink_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            link = root / "link"
            for target in (root, root / "missing"):
                with self.subTest(target=target):
                    link.symlink_to(target)
                    self.assertEqual(self.invoke([str(link)]).returncode, 2)
                    self.assertTrue(link.is_symlink())
                    link.unlink()

    def test_missing_tool_precedes_directory_creation(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "not-created"
            result = self.invoke([str(destination)], dict(os.environ, PATH=temp))
            self.assertEqual(result.returncode, 2)
            self.assertIn("Required tool missing: meson", result.stderr)
            self.assertFalse(destination.exists())

    def test_symlink_parent_rejected_without_creation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            alias = root / "alias"
            alias.symlink_to(root, target_is_directory=True)
            destination = alias / "not-created"
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"])
            for tool in ("meson", "ninja"):
                stub = root / tool
                stub.write_text("#!/bin/sh\nexit 99\n")
                stub.chmod(0o700)
            result = self.invoke([str(destination)], env)
            self.assertEqual(result.returncode, 2)
            self.assertIn("canonical", result.stderr)
            self.assertFalse(destination.exists())

    def test_wrong_archive_stops_before_extraction_or_build(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for tool in ("meson", "ninja"):
                stub = root / tool
                stub.write_text("#!/bin/sh\necho unexpected-build >&2\nexit 99\n")
                stub.chmod(0o700)
            bad = root / "corrupt.tar.bz2"
            bad.write_text("not a source archive")
            destination = root / "sdk"
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"],
                       PIPEWIRE_SOURCE_ARCHIVE=str(bad))
            result = self.invoke([str(destination)], env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("FAILED", result.stdout)
            self.assertNotIn("unexpected-build", result.stderr)
            self.assertFalse((destination / "source").exists())
            self.assertFalse((destination / "build").exists())

    def test_wrong_debian_delta_stops_before_any_extraction(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for tool in ("meson", "ninja", "tar", "patch"):
                stub = root / tool
                stub.write_text("#!/bin/sh\necho unexpected-extract-or-build >&2\nexit 99\n")
                stub.chmod(0o700)
            # Isolate ordering: first hash check is a contract stub, second
            # check is the actual sha256sum. No download or native build claim.
            sha = root / "sha256sum"
            sha.write_text(
                "#!/bin/sh\npayload=$(dd status=none)\n"
                "case \"$payload\" in\n"
                " *pipewire-1.6.9.tar.gz) echo source-hash-contract-stub;;\n"
                " *) printf '%s\\n' \"$payload\" | " + shutil.which("sha256sum") + " \"$@\";;\n"
                "esac\n")
            sha.chmod(0o700)
            bad = root / "corrupt.tar"
            bad.write_text("not a Debian source delta")
            destination = root / "sdk"
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"],
                       PIPEWIRE_SOURCE_ARCHIVE=str(bad), PIPEWIRE_DEBIAN_ARCHIVE=str(bad))
            result = self.invoke([str(destination)], env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("source-hash-contract-stub", result.stdout)
            self.assertIn("FAILED", result.stdout)
            self.assertNotIn("unexpected-extract-or-build", result.stderr)
            self.assertFalse((destination / "source").exists())
            self.assertFalse((destination / "build").exists())


@unittest.skipUnless(sys.platform == "linux", "requires Linux shell tooling")
class RuntimeCheckerContract(unittest.TestCase):
    def invoke(self, arguments, env=None):
        return subprocess.run([BASH, str(CHECKER), *arguments], env=env,
                              capture_output=True, text=True, timeout=15)

    def test_existing_output_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            marker = root / "keep.txt"
            marker.write_text("keep original output")
            self.assertEqual(self.invoke(["/missing-pw", "/missing-ei", temp]).returncode, 2)
            self.assertEqual(marker.read_text(), "keep original output")
            self.assertEqual(list(root.iterdir()), [marker])

    def test_noncanonical_sdk_rejected_without_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            alias = root / "alias"
            alias.symlink_to(root, target_is_directory=True)
            output = root / "not-created"
            result = self.invoke([str(alias), temp, str(output)])
            self.assertEqual(result.returncode, 2)
            self.assertFalse(output.exists())

    def test_missing_private_component_rejected_without_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            stub = root / "patchelf"
            stub.write_text("#!/bin/sh\necho unexpected-patch >&2\nexit 99\n")
            stub.chmod(0o700)
            output = root / "not-created"
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"])
            result = self.invoke([temp, temp, str(output)], env)
            self.assertEqual(result.returncode, 2)
            self.assertIn("Private component missing", result.stderr)
            self.assertNotIn("unexpected-patch", result.stderr)
            self.assertFalse(output.exists())

    def test_soname_link_escape_rejected_before_copy_or_compile(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            sdk = root / "sdk"
            (sdk / "lib").mkdir(parents=True)
            (sdk / "lib/libpipewire-0.3.so").write_text("inert link-boundary fixture")
            outside = root / "outside.so"
            outside.write_text("must not copy this")
            (sdk / "lib/libpipewire-0.3.so.0").symlink_to(outside)
            stub = root / "patchelf"
            stub.write_text("#!/bin/sh\necho unexpected-patch >&2\nexit 99\n")
            stub.chmod(0o700)
            output = root / "not-created"
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"])
            result = self.invoke([str(sdk), str(sdk), str(output)], env)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("unexpected-patch", result.stderr)
            self.assertFalse(output.exists())
            self.assertEqual(outside.read_text(), "must not copy this")


if __name__ == "__main__":
    unittest.main(verbosity=2)
