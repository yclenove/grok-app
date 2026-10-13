"""Retain copied notices and errors without inferring shipping exclusions."""
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-swiftshader-notices.py")))
ROOT = SUBJECT["ROOT"]
LLVM_HEADER = (b"// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.\n"
               b"// See https://llvm.org/LICENSE.txt for license information.\n"
               b"// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception\n\n#ifndef TEST\n")


class SwiftShaderNoticeTests(unittest.TestCase):
    def setUp(self):
        self.texts = {"Apache-2.0": b"Complete synthetic Apache text\n", "NCSA": b"Complete synthetic NCSA\n",
                      "LLVM-exception": b"Complete synthetic LLVM exception\n"}
        self.pins = {key: {"oid": hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest(),
                           "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                     for key, data in self.texts.items()}
        self.notices = [SUBJECT["leading_notice"](LLVM_HEADER, ROOT + "third_party/llvm-10.0/llvm/ADT/StringRef.h")]
        self.path = ROOT + "third_party/llvm-10.0/llvm/include/llvm/Support/LICENSE.TXT"
        self.payload = b"Copyright (C) 2004 eXtensible Systems, Inc.\nIllinois Open Source License\n"
        self.binding = {"sourcePath": self.path, "bytes": len(self.payload), "sha256": hashlib.sha256(self.payload).hexdigest()}
        self.inputs = {"files": {self.path: {"payload": self.payload}}}

    def fragment(self, texts=None):
        return SUBJECT["render_fragment"](self.notices, [self.binding], self.inputs,
                                           self.texts if texts is None else texts, self.pins)

    def test_complete_gn_selection_excludes_cpp_symlinks_and_other_roots(self):
        index = {ROOT + "BUILD.gn": {"mode": "100644", "gitObjectSha1": "a" * 40},
                 ROOT + "src/foo.gni": {"mode": "100755", "gitObjectSha1": "b" * 40},
                 ROOT + "src/link.gn": {"mode": "120000", "gitObjectSha1": "c" * 40},
                 ROOT + "src/foo.cpp": {"mode": "100644", "gitObjectSha1": "d" * 40},
                 "third_party/angle/BUILD.gn": {"mode": "100644", "gitObjectSha1": "e" * 40}}
        self.assertEqual(SUBJECT["gn_paths"](index), {ROOT + "BUILD.gn": "a" * 40, ROOT + "src/foo.gni": "b" * 40})

    def test_llvm_declaration_keeps_exception_and_entire_original_notice(self):
        row = self.notices[0]
        self.assertEqual(row["fullLicenseIds"], ["Apache-2.0", "LLVM-exception"])
        self.assertEqual(row["originalAttributionText"].encode(), LLVM_HEADER.split(b"\n\n")[0] + b"\n")
        self.assertFalse(row["wholeCopiedTreeCovered"])
        for data in (LLVM_HEADER.replace(b" WITH LLVM-exception", b""), LLVM_HEADER.replace(b"SPDX-License-Identifier", b"SPDX-License-Changed")):
            with self.assertRaises(ValueError):
                SUBJECT["leading_notice"](data, row["sourcePath"])

    def test_swiftshader_short_notice_cannot_omit_disclaimer(self):
        lines = ("Copyright 2019 The SwiftShader Authors. All Rights Reserved.",
                 "Licensed under the Apache License, Version 2.0", "http://www.apache.org/licenses/LICENSE-2.0",
                 'distributed under the License is distributed on an "AS IS" BASIS',
                 "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND", "limitations under the License.")
        data = "".join("// " + line + "\n" for line in lines).encode() + b"\n#include <foo>\n"
        row = SUBJECT["leading_notice"](data, ROOT + "src/Pipeline/ComputeProgram.cpp")
        self.assertEqual(row["fullLicenseIds"], ["Apache-2.0"])
        with self.assertRaises(ValueError):
            SUBJECT["leading_notice"](data.replace(b"WITHOUT WARRANTIES", b"omitted"), row["sourcePath"])

    def test_fragment_keeps_actual_copyright_special_notice_and_full_exception(self):
        original = copy.deepcopy((self.notices, self.inputs, self.texts))
        data = self.fragment()
        self.assertIn(self.payload, data)
        self.assertIn(self.notices[0]["originalAttributionText"].encode(), data)
        for text in self.texts.values():
            self.assertIn(text, data)
        self.assertIn(b"NOT FOR DISTRIBUTION", data)
        self.assertEqual((self.notices, self.inputs, self.texts), original)

    def test_fragment_rejects_missing_truncated_standard_or_changed_source(self):
        for texts in ({"Apache-2.0": self.texts["Apache-2.0"]}, {**self.texts, "LLVM-exception": self.texts["LLVM-exception"][:-1]}):
            with self.assertRaises(ValueError):
                self.fragment(texts)
        self.inputs["files"][self.path]["payload"] += b" "
        with self.assertRaisesRegex(ValueError, "actual complete source"):
            self.fragment()

    def test_fragment_budget_includes_headers_and_separators(self):
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(self.fragment()) - 1):
            with self.assertRaisesRegex(ValueError, "bounded"):
                self.fragment()

    def test_all_eight_errors_and_gitlink_remain_without_exclusion(self):
        errors = [{"directory": path, "metadata": None, "error": "original missing metadata"}
                  for path in sorted(SUBJECT["ERROR_DIRECTORIES"])]
        index = {ROOT + "third_party/libbacktrace/src": {"mode": "160000", "gitObjectSha1": "f" * 40}}
        result = SUBJECT["error_dispositions"](errors, [self.binding], index, {"copiedLibraryReferences": []})
        self.assertEqual([row["originalDirectoryError"] for row in result], errors)
        lib = next(row for row in result if row["gitlinks"])
        self.assertEqual(lib["disposition"], "unresolved_source_or_shipping_obligation")
        for row in result:
            self.assertFalse(row["rootApacheCoversCopiedCode"])
            self.assertFalse(row["gnReferenceAbsenceMeansNotShipped"])
            self.assertFalse(row["originalErrorRemoved"])
        with self.assertRaises(ValueError):
            SUBJECT["error_dispositions"](errors[:-1], [], index, {"copiedLibraryReferences": []})

    def test_existing_and_escaping_output_fail_before_loading_sources(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-swift-notice-") as name:
            source = Path(name)
            output = source / "swiftshader-notices-01"
            output.mkdir()
            marker = output / "keep"
            marker.write_bytes(b"original")
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None):
                for invalid in (source / "../swiftshader-notices-01", source / "foreign"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, invalid)
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, output)
            self.assertEqual(marker.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
