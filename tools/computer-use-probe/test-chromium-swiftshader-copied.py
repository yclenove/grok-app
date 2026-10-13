"""Special license blocks stay complete; debug config cannot prove exclusions."""
import copy
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-swiftshader-copied.py")))
ROOT = SUBJECT["ROOT"]
MD5 = (b"/* This code has its original special terms.\n"
       b"Copyright (c) 2001 Alexander Peslyak\nNo copyright is claimed.\n"
       b"If the public domain is deemed null and void, the fallback applies.\n"
       b"Redistribution and use in source and binary forms, with or without\n"
       b"modification, are permitted.\nThere's ABSOLUTELY NO WARRANTY, express or implied.\n*/\n")
BACKTRACE = (b"/* Copyright (C) 2012-2016 Free Software Foundation, Inc.\n"
             b"Written by Ian Lance Taylor, Google.\n(1) Redistributions of source code\n"
             b"(2) Redistributions in binary form\n(3) The name of the author\n"
             b"THIS SOFTWARE IS PROVIDED BY THE AUTHOR\nPOSSIBILITY OF SUCH DAMAGE. */\n")


class CopiedNoticeTests(unittest.TestCase):
    def setUp(self):
        self.blobs = {
            ROOT + ".gitmodules": b'[submodule "third_party/libbacktrace/src"]\npath = third_party/libbacktrace/src\nurl = https://github.com/ianlancetaylor/libbacktrace.git\n',
            ROOT + "CMakeLists.txt": b'option_if_not_defined(REACTOR_EMIT_DEBUG_INFO "Emit debug info for JIT functions" FALSE)\n'
                b'if(REACTOR_EMIT_DEBUG_INFO)\n    InitSubmodule(libbacktrace ${THIRD_PARTY_DIR}/libbacktrace/src)\nendif()\n'
                b'if(REACTOR_EMIT_DEBUG_INFO)\n    add_subdirectory(${THIRD_PARTY_DIR}/libbacktrace EXCLUDE_FROM_ALL)\n'
                b'    add_subdirectory(${THIRD_PARTY_DIR}/boost EXCLUDE_FROM_ALL)\nendif()\n',
            ROOT + "third_party/boost/CMakeLists.txt": b'set(BOOST_VER 1.78.0)\n'
                b'set(BOOST_HASH_TARGZ "94ced8b72956591c4775ae2207a9763d3600b30d9d7446562c552f0a14a63be7")\n'
                b'set(BOOST_HASH_ZIP   "f22143b5528e081123c3c5ed437e92f648fe69748e95fa6e2bd41484e2986cc3")\n'
                b'            file(DOWNLOAD ${BOOST_URL} ${BOOST_ARCHIVE} EXPECTED_HASH SHA256=${BOOST_HASH})\n',
            ROOT + "third_party/libbacktrace/CMakeLists.txt": b'elseif(APPLE)\n    message(FATAL_ERROR "libbacktrace does not support mach-o yet")\n',
        }
        self.index = {ROOT + "third_party/libbacktrace/src": {"mode": "160000", "kind": "commit", "gitObjectSha1": SUBJECT["LIBBACKTRACE_REVISION"]}}
        for platform in ("darwin", "linux"):
            for name in ("config.h", "backtrace-supported.h"):
                path = ROOT + "third_party/libbacktrace/config/" + platform + "/include/" + name
                self.index[path] = {"mode": "100644"}
                self.blobs[path] = BACKTRACE if name == "backtrace-supported.h" else b"/* generated config */\n"

    def libraries(self, gn=None):
        return SUBJECT["library_sources"](self.blobs, self.index, {} if gn is None else gn)

    def test_md5_public_domain_fallback_and_disclaimer_are_retained(self):
        row = SUBJECT["inline_notice"](MD5 + b"#include <foo>\n", ROOT + "third_party/llvm-10.0/llvm/lib/Support/MD5.cpp")
        self.assertEqual(row["originalAttributionText"].encode(), MD5.rstrip(b"\n"))
        self.assertEqual(row["noticeKind"], "original_md5_public_domain_and_fallback_permission")
        for text in (b"Copyright (c) 2001 Alexander Peslyak", b"public domain is deemed null and void", b"ABSOLUTELY NO WARRANTY"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                SUBJECT["inline_notice"](MD5.replace(text, b"omitted"), row["sourcePath"])

    def test_backtrace_binary_condition_author_and_disclaimer_cannot_be_omitted(self):
        row = SUBJECT["inline_notice"](BACKTRACE, ROOT + "third_party/libbacktrace/config/linux/include/backtrace-supported.h")
        self.assertTrue(row["fullPermissionIsInline"])
        self.assertFalse(row["wholeComponentNoticeCovered"])
        for text in (b"(2) Redistributions in binary form", b"Written by Ian Lance Taylor, Google.", b"POSSIBILITY OF SUCH DAMAGE."):
            with self.subTest(text=text), self.assertRaises(ValueError):
                SUBJECT["inline_notice"](BACKTRACE.replace(text, b"omitted"), row["sourcePath"])

    def test_fragment_preserves_exact_special_text_and_rejects_mutated_attribution(self):
        notices = [SUBJECT["inline_notice"](MD5, ROOT + "third_party/llvm-subzero/include/llvm/Support/MD5.h"),
                   SUBJECT["inline_notice"](BACKTRACE, ROOT + "third_party/libbacktrace/config/darwin/include/backtrace-supported.h")]
        original = copy.deepcopy(notices)
        data = SUBJECT["render_fragment"](notices)
        self.assertIn(MD5.rstrip(), data)
        self.assertIn(BACKTRACE.rstrip(), data)
        self.assertEqual(notices, original)
        notices[0]["originalAttributionText"] += " changed"
        with self.assertRaises(ValueError):
            SUBJECT["render_fragment"](notices)

    def test_incomplete_or_foreign_inline_notices_are_rejected_and_budget_is_bounded(self):
        with self.assertRaises(ValueError):
            SUBJECT["inline_notice"](MD5, ROOT + "third_party/unknown/LICENSE")
        with self.assertRaises(ValueError):
            SUBJECT["inline_notice"](MD5[:-3], ROOT + "MD5.h")
        notices = [SUBJECT["inline_notice"](MD5, ROOT + "MD5.h")]
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(MD5)), self.assertRaises(ValueError):
            SUBJECT["render_fragment"](notices)

    def test_debug_branch_archive_hash_and_gitlink_are_evidence_not_exclusions(self):
        row = self.libraries()
        self.assertEqual(row["libbacktraceSource"]["treeKind"], "gitlink")
        self.assertEqual(row["boostSource"]["version"], "1.78.0")
        self.assertFalse(row["binaryExclusionsProven"])
        self.assertFalse(row["gnReferenceAbsenceMeansNotShipped"])
        self.assertFalse(row["cmakeExecuted"])
        with_ref = self.libraries({ROOT + "BUILD.gn": b'include_dirs = [ "third_party/boost" ]\n'})
        self.assertEqual(len(with_ref["gnReferences"]), 1)
        self.assertFalse(with_ref["effectiveGnMeasured"])

    def test_foreign_submodule_or_regular_file_cannot_replace_exact_gitlink(self):
        key = ROOT + ".gitmodules"
        self.blobs[key] = self.blobs[key].replace(b"ianlancetaylor", b"foreign")
        with self.assertRaises(ValueError):
            self.libraries()
        self.setUp()
        self.index[ROOT + "third_party/libbacktrace/src"]["mode"] = "100644"
        with self.assertRaises(ValueError):
            self.libraries()

    def test_download_pin_change_or_incomplete_config_tree_is_rejected(self):
        key = ROOT + "third_party/boost/CMakeLists.txt"
        self.blobs[key] = self.blobs[key].replace(b"set(BOOST_VER 1.78.0)", b"set(BOOST_VER 1.79.0)")
        with self.assertRaises(ValueError):
            self.libraries()
        self.setUp()
        del self.index[ROOT + "third_party/libbacktrace/config/linux/include/config.h"]
        with self.assertRaises(ValueError):
            self.libraries()

    def test_output_cannot_escape_or_overwrite_existing_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-special-notice-") as name:
            source = Path(name)
            output = source / "swiftshader-copied-notices-01"
            output.mkdir()
            marker = output / "keep"
            marker.write_bytes(b"original")
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, output)
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](source, source / "../swiftshader-copied-notices-01")
            self.assertEqual(marker.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
