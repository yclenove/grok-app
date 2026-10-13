#!/usr/bin/env python3
"""Policy Data Integrity Validator for Grok Computer Use v2.

Validates:
- denylist.toml: TOML syntax, platforms, unique IDs, valid categories, minimum counts, match kinds.
- sensitive_terms/*.txt: 15 required locales, 10 categories per locale, >=2 terms per category.
- File integrity: UTF-8 encoding, LF line endings, EOF newline, no consecutive blank lines, no control characters.
"""

from __future__ import annotations

import argparse
import pathlib
import sys
import tomllib
from typing import List, Dict, Set

REQUIRED_LOCALES = {
    "de",
    "en",
    "es",
    "fil",
    "fr",
    "id",
    "it",
    "ja",
    "ko",
    "pt-BR",
    "ru",
    "ta",
    "uk",
    "zh",
    "zh-TW",
}

ALLOWED_DENYLIST_CATEGORIES = {
    "grok_self",
    "terminal",
    "credentials",
    "security_prompt",
    "security_software",
}

ALLOWED_MATCH_KINDS = {"exact"}

REQUIRED_PLATFORMS = {"macos", "windows", "linux"}
MIN_ENTRIES_PER_PLATFORM = 10

REQUIRED_SENSITIVE_CATEGORIES = {
    "send",
    "pay",
    "delete",
    "submit",
    "transfer",
    "sign",
    "publish",
    "confirm_purchase",
    "password_change",
    "logout_all",
}
MIN_TERMS_PER_CATEGORY = 2


def check_file_hygiene(path: pathlib.Path, errors: List[str]) -> bytes | None:
    """Validate file encoding, EOF newline, control characters, and line spacing."""
    try:
        raw = path.read_bytes()
    except OSError as e:
        errors.append(f"{path}: failed to read file: {e}")
        return None

    if not raw:
        errors.append(f"{path}: file is empty")
        return None

    if not raw.endswith(b"\n"):
        errors.append(f"{path}: file must end with a newline (\\n)")

    if raw.endswith(b"\n\n"):
        errors.append(f"{path}: file must not end with trailing blank lines")

    if b"\r" in raw:
        errors.append(f"{path}: contains Windows CRLF or CR line endings; use LF (\\n)")

    # Check for invalid control characters (allow \t 0x09 and \n 0x0A)
    for byte_offset, byte_val in enumerate(raw):
        if (byte_val < 32 and byte_val not in (9, 10)) or byte_val == 127:
            errors.append(
                f"{path}: contains forbidden control character 0x{byte_val:02X} at byte offset {byte_offset}"
            )
            break

    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as e:
        errors.append(f"{path}: invalid UTF-8 encoding: {e}")
        return None

    # Check for consecutive blank lines
    lines = text.split("\n")
    consecutive_empty = 0
    for line_num, line in enumerate(lines, start=1):
        if not line.strip():
            consecutive_empty += 1
            if consecutive_empty > 1:
                errors.append(
                    f"{path}:{line_num}: consecutive empty lines detected"
                )
                break
        else:
            consecutive_empty = 0

    return raw


def validate_denylist(path: pathlib.Path, errors: List[str]) -> None:
    """Validate denylist.toml format, fields, uniqueness, and constraints."""
    raw = check_file_hygiene(path, errors)
    if raw is None:
        return

    try:
        data = tomllib.loads(raw.decode("utf-8"))
    except tomllib.TOMLDecodeError as e:
        errors.append(f"{path}: TOML decode error: {e}")
        return

    for platform in REQUIRED_PLATFORMS:
        if platform not in data:
            errors.append(f"{path}: missing required platform section '{platform}'")
            continue

        entries = data[platform]
        if not isinstance(entries, list):
            errors.append(
                f"{path}: platform '{platform}' must be a list/array of tables"
            )
            continue

        if len(entries) < MIN_ENTRIES_PER_PLATFORM:
            errors.append(
                f"{path}: platform '{platform}' has {len(entries)} entries, expected at least {MIN_ENTRIES_PER_PLATFORM}"
            )

        seen_ids: Set[str] = set()
        for idx, entry in enumerate(entries):
            if not isinstance(entry, dict):
                errors.append(
                    f"{path}: {platform}[{idx}] must be a table/dictionary"
                )
                continue

            entry_id = entry.get("id")
            category = entry.get("category")
            match_kind = entry.get("match")

            if not isinstance(entry_id, str) or not entry_id.strip():
                errors.append(
                    f"{path}: {platform}[{idx}] has missing or empty 'id'"
                )
                continue

            if entry_id in seen_ids:
                errors.append(
                    f"{path}: {platform} contains duplicate id '{entry_id}'"
                )
            seen_ids.add(entry_id)

            if category not in ALLOWED_DENYLIST_CATEGORIES:
                errors.append(
                    f"{path}: {platform}[{idx}] id '{entry_id}' has invalid category '{category}' "
                    f"(expected one of {sorted(ALLOWED_DENYLIST_CATEGORIES)})"
                )

            if match_kind not in ALLOWED_MATCH_KINDS:
                errors.append(
                    f"{path}: {platform}[{idx}] id '{entry_id}' has invalid match kind '{match_kind}' "
                    f"(expected one of {sorted(ALLOWED_MATCH_KINDS)})"
                )

            # Windows executables should be lowercase and end with .exe
            if platform == "windows":
                if entry_id != entry_id.lower():
                    errors.append(
                        f"{path}: windows entry '{entry_id}' must be lowercase"
                    )
                if not entry_id.endswith(".exe"):
                    errors.append(
                        f"{path}: windows entry '{entry_id}' must end with '.exe'"
                    )


def validate_sensitive_terms(dir_path: pathlib.Path, errors: List[str]) -> None:
    """Validate multilingual sensitive terms dictionaries."""
    if not dir_path.is_dir():
        errors.append(f"{dir_path}: sensitive_terms directory not found")
        return

    existing_files = {p.stem: p for p in dir_path.glob("*.txt")}
    missing_locales = REQUIRED_LOCALES - set(existing_files.keys())
    if missing_locales:
        errors.append(
            f"{dir_path}: missing sensitive term files for locales: {sorted(missing_locales)}"
        )

    for locale, path in sorted(existing_files.items()):
        raw = check_file_hygiene(path, errors)
        if raw is None:
            continue

        text = raw.decode("utf-8")
        current_category: str | None = None
        category_terms: Dict[str, List[str]] = {
            cat: [] for cat in REQUIRED_SENSITIVE_CATEGORIES
        }
        seen_terms_in_file: Set[str] = set()

        for line_num, raw_line in enumerate(text.splitlines(), start=1):
            line = raw_line.strip()
            if not line:
                continue

            if line.startswith("#"):
                # Parse section header e.g. "# send" or "# send - description"
                comment_body = line.lstrip("#").strip()
                tokens = comment_body.split()
                header_category = tokens[0] if tokens else ""
                if header_category in REQUIRED_SENSITIVE_CATEGORIES:
                    current_category = header_category
                continue

            if current_category is None:
                errors.append(
                    f"{path}:{line_num}: term '{line}' defined before any category header"
                )
                continue

            if raw_line != line:
                errors.append(
                    f"{path}:{line_num}: term has leading/trailing whitespace: '{raw_line}'"
                )

            term_key = line.lower()
            if term_key in seen_terms_in_file:
                errors.append(
                    f"{path}:{line_num}: duplicate term '{line}' in {locale}.txt"
                )
            seen_terms_in_file.add(term_key)

            category_terms[current_category].append(line)

        # Verify each required category has at least MIN_TERMS_PER_CATEGORY
        for cat in sorted(REQUIRED_SENSITIVE_CATEGORIES):
            terms = category_terms.get(cat, [])
            if len(terms) < MIN_TERMS_PER_CATEGORY:
                errors.append(
                    f"{path}: category '{cat}' has {len(terms)} terms, "
                    f"expected at least {MIN_TERMS_PER_CATEGORY}"
                )


def validate_policy_data(data_dir: pathlib.Path) -> List[str]:
    """Run all validation checks against the policy data directory."""
    errors: List[str] = []

    if not data_dir.is_dir():
        errors.append(f"Data directory does not exist: {data_dir}")
        return errors

    denylist_path = data_dir / "denylist.toml"
    if not denylist_path.is_file():
        errors.append(f"Missing denylist file: {denylist_path}")
    else:
        validate_denylist(denylist_path, errors)

    terms_dir = data_dir / "sensitive_terms"
    validate_sensitive_terms(terms_dir, errors)

    readme_path = data_dir / "README.md"
    if not readme_path.is_file():
        errors.append(f"Missing documentation file: {readme_path}")
    else:
        check_file_hygiene(readme_path, errors)

    return errors


def default_data_dir() -> pathlib.Path:
    """Locate the default policy data directory."""
    script_dir = pathlib.Path(__file__).resolve().parent
    repo_candidate = script_dir.parent / "crates" / "gcu-core" / "data"
    if repo_candidate.exists():
        return repo_candidate
    return pathlib.Path("computer-use/crates/gcu-core/data").resolve()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate Grok Computer Use v2 policy data files."
    )
    parser.add_argument(
        "--data-dir",
        type=pathlib.Path,
        default=default_data_dir(),
        help="Path to the gcu-core/data directory (default: %(default)s)",
    )
    args = parser.parse_args()

    data_dir = args.data_dir.resolve()
    errors = validate_policy_data(data_dir)

    if errors:
        sys.stderr.write(
            f"ERROR: Policy data validation failed with {len(errors)} error(s):\n"
        )
        for err in errors:
            sys.stderr.write(f"  - {err}\n")
        return 1

    print(f"Policy data validation passed for: {data_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
