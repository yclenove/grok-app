#!/usr/bin/env python3
"""Unit tests for check_policy_data.py policy data validator."""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

# Import the validator module directly
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import check_policy_data  # noqa: E402

class TestCheckPolicyData(unittest.TestCase):
    """Test suite covering positive and negative validation cases."""

    def setUp(self) -> None:
        self.repo_root = pathlib.Path(__file__).resolve().parent.parent.parent
        self.real_data_dir = (
            self.repo_root / "computer-use" / "crates" / "gcu-core" / "data"
        )
        self.script_path = (
            self.repo_root / "computer-use" / "scripts" / "check_policy_data.py"
        )

    def test_real_policy_data_passes(self) -> None:
        """The actual production policy data directory must pass with zero errors."""
        errors = check_policy_data.validate_policy_data(self.real_data_dir)
        self.assertEqual(
            errors,
            [],
            f"Expected zero validation errors on real data, got:\n"
            + "\n".join(errors),
        )

    def test_cli_invocation_success(self) -> None:
        """Invoking the script via Python CLI on valid data should exit with 0."""
        proc = subprocess.run(
            [
                sys.executable,
                "-I",
                str(self.script_path),
                "--data-dir",
                str(self.real_data_dir),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(
            proc.returncode,
            0,
            f"CLI failed with stderr:\n{proc.stderr}\nstdout:\n{proc.stdout}",
        )
        self.assertIn("Policy data validation passed", proc.stdout)

    def test_cli_invocation_failure(self) -> None:
        """Invoking the script via Python CLI on empty dir should exit with non-zero."""
        with tempfile.TemporaryDirectory() as tmpdir:
            proc = subprocess.run(
                [
                    sys.executable,
                    "-I",
                    str(self.script_path),
                    "--data-dir",
                    tmpdir,
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(proc.returncode, 0)
            self.assertIn("ERROR", proc.stderr)

    def _create_mock_valid_dataset(self, target_dir: pathlib.Path) -> None:
        """Copy the valid real dataset to a temporary directory."""
        shutil.copytree(self.real_data_dir, target_dir, dirs_exist_ok=True)

    def test_missing_denylist_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            (d / "denylist.toml").unlink()

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(any("Missing denylist file" in e for e in errors))

    def test_invalid_toml_syntax(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            (d / "denylist.toml").write_bytes(b"invalid = [toml unclosed\n")

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(any("TOML decode error" in e for e in errors))

    def test_missing_platform_in_denylist(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            # Remove [linux] section
            content = (d / "denylist.toml").read_text(encoding="utf-8")
            cut_idx = content.find("# Linux")
            truncated = content[:cut_idx]
            (d / "denylist.toml").write_text(truncated, encoding="utf-8")

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("missing required platform section 'linux'" in e for e in errors)
            )

    def test_duplicate_id_in_denylist(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            # Append duplicate entry to macos
            dup_entry = (
                '\n[[macos]]\nid = "com.apple.Terminal"\ncategory = "terminal"\nmatch = "exact"\n'
            )
            (d / "denylist.toml").write_text(
                (d / "denylist.toml").read_text(encoding="utf-8") + dup_entry,
                encoding="utf-8",
            )

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("duplicate id 'com.apple.Terminal'" in e for e in errors)
            )

    def test_invalid_category_in_denylist(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            bad_entry = (
                '\n[[macos]]\nid = "com.bad.test"\ncategory = "unknown_category"\nmatch = "exact"\n'
            )
            (d / "denylist.toml").write_text(
                (d / "denylist.toml").read_text(encoding="utf-8") + bad_entry,
                encoding="utf-8",
            )

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("invalid category 'unknown_category'" in e for e in errors)
            )

    def test_invalid_match_in_denylist(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            bad_entry = (
                '\n[[macos]]\nid = "com.bad.test"\ncategory = "terminal"\nmatch = "regex"\n'
            )
            (d / "denylist.toml").write_text(
                (d / "denylist.toml").read_text(encoding="utf-8") + bad_entry,
                encoding="utf-8",
            )

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("invalid match kind 'regex'" in e for e in errors)
            )

    def test_windows_entry_not_lowercase(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            bad_entry = (
                '\n[[windows]]\nid = "UPPERCASE.EXE"\ncategory = "terminal"\nmatch = "exact"\n'
            )
            (d / "denylist.toml").write_text(
                (d / "denylist.toml").read_text(encoding="utf-8") + bad_entry,
                encoding="utf-8",
            )

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("must be lowercase" in e for e in errors)
            )

    def test_windows_entry_missing_exe_extension(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            bad_entry = (
                '\n[[windows]]\nid = "noextension"\ncategory = "terminal"\nmatch = "exact"\n'
            )
            (d / "denylist.toml").write_text(
                (d / "denylist.toml").read_text(encoding="utf-8") + bad_entry,
                encoding="utf-8",
            )

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("must end with '.exe'" in e for e in errors)
            )

    def test_missing_locale_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            (d / "sensitive_terms" / "ta.txt").unlink()

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("missing sensitive term files for locales" in e and "ta" in e for e in errors)
            )

    def test_too_few_terms_in_category(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            # Reduce 'send' category in en.txt to 1 term
            content = (d / "sensitive_terms" / "en.txt").read_text(encoding="utf-8")
            content = content.replace("send message\n", "")
            (d / "sensitive_terms" / "en.txt").write_text(content, encoding="utf-8")

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("category 'send' has 1 terms, expected at least 2" in e for e in errors)
            )

    def test_duplicate_terms_in_locale(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            content = (d / "sensitive_terms" / "en.txt").read_text(encoding="utf-8")
            content += "send\n"
            (d / "sensitive_terms" / "en.txt").write_text(content, encoding="utf-8")

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("duplicate term 'send'" in e for e in errors)
            )

    def test_missing_newline_at_eof(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            raw = (d / "sensitive_terms" / "en.txt").read_bytes()
            if raw.endswith(b"\n"):
                raw = raw[:-1]
            (d / "sensitive_terms" / "en.txt").write_bytes(raw)

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("must end with a newline" in e for e in errors)
            )

    def test_consecutive_blank_lines(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            content = (d / "sensitive_terms" / "en.txt").read_text(encoding="utf-8")
            content = content.replace("\n\n# pay", "\n\n\n# pay")
            (d / "sensitive_terms" / "en.txt").write_text(content, encoding="utf-8")

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("consecutive empty lines detected" in e for e in errors)
            )

    def test_forbidden_control_characters(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            raw = (d / "sensitive_terms" / "en.txt").read_bytes()
            raw = raw + b"\x00\n"
            (d / "sensitive_terms" / "en.txt").write_bytes(raw)

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("contains forbidden control character 0x00" in e for e in errors)
            )

    def test_missing_readme(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._create_mock_valid_dataset(d)
            (d / "README.md").unlink()

            errors = check_policy_data.validate_policy_data(d)
            self.assertTrue(
                any("Missing documentation file" in e for e in errors)
            )


if __name__ == "__main__":
    unittest.main()
