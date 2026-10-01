#!/usr/bin/env python3
"""Runner/SDK guard contracts; these are not native Wayland acceptance."""
import importlib.util
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import shutil
import uuid
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("wayland_runner", ROOT / "run-computer-use-wayland-tests.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class SuiteContract(unittest.TestCase):
    def valid(self, suffix="105 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"):
        return "test " + runner.PARENT_TEST + " ... ok\ntest result: ok. " + suffix

    def test_complete_suite(self):
        self.assertEqual(runner.verify_suite(self.valid()), 105)

    def test_ignored_suite_rejected(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid().replace("0 ignored", "26 ignored"))

    def test_filtered_suite_rejected(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid().replace("0 filtered out", "1 filtered out"))

    def test_zero_tests_rejected(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid().replace("105 passed", "0 passed"))

    def test_parent_integration_required(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid().replace(runner.PARENT_TEST, "some_other_test"))

    def test_panic_cannot_be_hidden_by_summary(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid() + "\npanicked at source.rs:1")

    def test_stderr_executor_panic_cannot_be_hidden_by_green_stdout(self):
        with self.assertRaisesRegex(RuntimeError, "contains a failure"):
            runner.verify_suite(self.valid(),
                                "thread 'zbus::Connection executor' panicked at fixture.rs:89")

    def test_stderr_native_crash_cannot_be_hidden_by_green_stdout(self):
        with self.assertRaisesRegex(RuntimeError, "contains a failure"):
            runner.verify_suite(self.valid(), "child terminated by SIGSEGV")

    def test_healthy_wayland_diagnostics_are_allowed(self):
        self.assertEqual(runner.verify_suite(self.valid(),
                                            "[123] wl_display#1.sync(new id wl_callback#3)"), 105)

    def test_duplicate_summary_rejected(self):
        with self.assertRaises(RuntimeError):
            runner.verify_suite(self.valid() + "\n" + self.valid())


class OwnershipContract(unittest.TestCase):
    def test_environment_is_private(self):
        ambient = {key: "user-session" for key in
                   ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS",
                    "AT_SPI_BUS_ADDRESS", "SESSION_MANAGER", "XAUTHORITY")}
        ambient["GROK_CU_PW_SOURCE"] = "/private/pw-source"
        with patch.dict(os.environ, ambient, clear=True):
            env = runner.fixture_environment(Path("/private/runtime"), "/private/Xwayland")
        for key in ambient:
            if key != "GROK_CU_PW_SOURCE":
                self.assertNotIn(key, env)
        self.assertEqual(env["XDG_RUNTIME_DIR"], "/private/runtime")
        self.assertEqual(env["GDK_BACKEND"], "wayland")
        self.assertEqual(env["WLR_BACKENDS"], "headless")
        self.assertEqual(env["GROK_CU_PW_SOURCE"], "/private/pw-source")

    def test_join_original_live_child(self):
        child = Mock(pid=42)
        child.poll.return_value = None
        with patch.object(runner.os, "killpg") as kill:
            runner.join_child(child)
        kill.assert_called_once_with(42, runner.signal.SIGTERM)
        child.wait.assert_called_once_with(timeout=5)

    def test_observation_timeout_does_not_replace_child(self):
        child = Mock(pid=42)
        child.poll.return_value = None
        child.wait.side_effect = [subprocess.TimeoutExpired("owned", 5), 0]
        with patch.object(runner.os, "killpg") as kill:
            runner.join_child(child)
        self.assertEqual([c.args[0] for c in kill.call_args_list], [42, 42])
        self.assertEqual(child.wait.call_count, 2)

    def test_terminal_child_still_waited(self):
        child = Mock(pid=42)
        child.poll.return_value = 0
        with patch.object(runner.os, "killpg") as kill:
            runner.join_child(child)
        kill.assert_not_called()
        child.wait.assert_called_once_with(timeout=5)

    def test_missing_pid_namespace_rejected_before_side_effects(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp) / "not-created"
            command = [sys.executable, str(ROOT / "run-computer-use-wayland-tests.py"),
                       "--test-binary", "/missing", "--evidence", str(out)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("private PID namespace", result.stderr)
            self.assertFalse(out.exists())

    @unittest.skipUnless(sys.platform == "linux", "owned native archive path is Linux")
    def test_keyboard_without_pointer_report_is_archived_from_exact_owned_fixture(self):
        original = Path("/var/tmp") / ("grok-cu-pw-" + uuid.uuid4().hex)
        original.mkdir(mode=0o700)
        try:
            payload = '{"keyboardApplied":true,"pointerDenied":true,"actualNativeGnome":false}\n'
            (original / "keyboard-without-pointer.json").write_text(payload)
            with tempfile.TemporaryDirectory() as temp:
                out = Path(temp)
                (out / "wayland-tests.log").write_text("owned PipeWire logs: " + str(original))
                audit = runner.archive_fixtures(out)
                self.assertEqual(audit["paths"], [str(original)])
                self.assertEqual(audit["missing"], [])
                archived = out / "fixtures" / original.name / "keyboard-without-pointer.json"
                self.assertEqual(archived.read_text(), payload)
        finally:
            shutil.rmtree(original)

    @unittest.skipUnless(sys.platform == "linux", "owned native archive path is Linux")
    def test_gnome_owner_revocation_report_is_archived_from_exact_owned_fixture(self):
        original = Path("/var/tmp") / ("grok-cu-pw-" + uuid.uuid4().hex)
        original.mkdir(mode=0o700)
        try:
            payload = '{"nativeJoined":true,"actualNativeGnome":false}\n'
            (original / "gnome-owner-revocation.json").write_text(payload)
            with tempfile.TemporaryDirectory() as temp:
                out = Path(temp)
                (out / "wayland-tests.log").write_text("owned PipeWire logs: " + str(original))
                audit = runner.archive_fixtures(out)
                self.assertEqual(audit["paths"], [str(original)])
                self.assertEqual(audit["missing"], [])
                archived = out / "fixtures" / original.name / "gnome-owner-revocation.json"
                self.assertEqual(archived.read_text(), payload)
        finally:
            shutil.rmtree(original)

    @unittest.skipUnless(sys.platform == "linux", "owned native archive path is Linux")
    def test_policy_revocation_report_is_archived_from_exact_owned_fixture(self):
        original = Path("/var/tmp") / ("grok-cu-pw-" + uuid.uuid4().hex)
        original.mkdir(mode=0o700)
        try:
            payload = '{"nativeJoined":true,"actualNativeGnome":false}\n'
            (original / "policy-revocation.json").write_text(payload)
            with tempfile.TemporaryDirectory() as temp:
                out = Path(temp)
                (out / "wayland-tests.log").write_text("owned PipeWire logs: " + str(original))
                audit = runner.archive_fixtures(out)
                self.assertEqual(audit["paths"], [str(original)])
                self.assertEqual(audit["missing"], [])
                archived = out / "fixtures" / original.name / "policy-revocation.json"
                self.assertEqual(archived.read_text(), payload)
                self.assertTrue(json.loads(archived.read_text())["nativeJoined"])
        finally:
            shutil.rmtree(original)


@unittest.skipUnless(sys.platform == "linux", "shell SDK guards run on Linux")
class SdkContract(unittest.TestCase):
    def check(self, pipewire_old=False, ei_old=False, archive=True):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            sdk = root / "sdk"
            sdk.mkdir()
            if archive:
                (sdk / "libei.a").write_text("inert contract fixture, not a native archive")
            pkg = root / "pkg-config"
            pkg.write_text(
                "#!/bin/sh\n"
                "case \"$*\" in\n"
                " *--atleast-version=0.3.65*) exit " + str(int(pipewire_old)) + ";;\n"
                " *--atleast-version=1.5*) exit " + str(int(ei_old)) + ";;\n"
                " *--variable=libdir*) printf '%s\\n' \"$TEST_SDK\";;\n"
                " *--modversion*) echo fixture-version;;\n"
                "esac\n")
            pkg.chmod(0o700)
            env = dict(os.environ, PATH=temp + ":" + os.environ["PATH"], TEST_SDK=str(sdk))
            return subprocess.run(["bash", str(ROOT / "check-computer-use-linux-sdk.sh")],
                                  env=env, text=True, capture_output=True)

    def test_sdk_guard_accepts_declared_requirements(self):
        self.assertEqual(self.check().returncode, 0)

    def test_old_pipewire_is_explicit_failure(self):
        result = self.check(pipewire_old=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("0.3.48", result.stderr)

    def test_old_libei_is_explicit_failure(self):
        result = self.check(ei_old=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("private static libei", result.stderr)

    def test_dynamic_only_libei_is_rejected(self):
        result = self.check(archive=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("static archive missing", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
