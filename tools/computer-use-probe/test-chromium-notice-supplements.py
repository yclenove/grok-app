"""Copied attributions, complete license bytes and original omission evidence."""
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-notice-supplements.py")))


def oid(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


class NoticeSupplementTests(unittest.TestCase):
    def test_header_selection_covers_api_subdirectories_and_exact_khronos_tree_only(self):
        prefix = SUBJECT["ANGLE"]
        paths = [prefix + "include/" + name + "/api.h" for name in SUBJECT["PINS"]["API_DIRECTORIES"]]
        paths.append(prefix + "src/third_party/khronos/GL/api.h")
        extra = [prefix + "include/GLSLANG/api.h", prefix + "include/CL/README.md", "third_party/other/include/CL/api.h"]
        index = {path: {"mode": "100644", "gitObjectSha1": "a" * 40} for path in paths + extra}
        index[prefix + "include/CL/link.h"] = {"mode": "120000", "gitObjectSha1": "b" * 40}
        index[prefix + "include/CL/submodule.h"] = {"mode": "160000", "gitObjectSha1": "b" * 40}
        self.assertEqual(set(SUBJECT["header_paths"](index)), set(paths))

    def test_spdx_keeps_actual_copyright_and_original_block_verbatim(self):
        block = "/*\n** Copyright 2007-2020 The Khronos Group Inc.\n** SPDX-License-Identifier: Apache-2.0\n*/"
        row = SUBJECT["original_attribution"](("#ifndef API\n" + block + "\n#define API\n").encode(), "api.h")
        self.assertEqual(row["originalAttributionText"], block)
        self.assertEqual(row["copyrightLines"], ["** Copyright 2007-2020 The Khronos Group Inc."])
        self.assertEqual(row["line"], 2)
        self.assertEqual(row["licenseId"], "Apache-2.0")
        self.assertEqual(row["attributionSha256"], hashlib.sha256(block.encode()).hexdigest())

    def test_unknown_multiple_or_lost_declarations_are_not_silently_normalized(self):
        block = b"/* Copyright 2020 The Khronos Group Inc.\nSPDX-License-Identifier: MIT */"
        for data in (block.replace(b"MIT", b"Unknown"), block + b"\n/* Copyright OTHER */", block.replace(b"Copyright", b"Removed"),
                     block.replace(b"The Khronos Group Inc.", b"Other owner"), block + b"\n/* SPDX-License-Identifier: Apache-2.0 */"):
            with self.subTest(data=data), self.assertRaises(ValueError):
                SUBJECT["original_attribution"](data, "api.h")

    def test_short_apache_notice_needs_the_original_disclaimer_as_well_as_name(self):
        parts = ("Copyright 2008 The Khronos Group Inc.", "Licensed under the Apache License, Version 2.0",
                 "you may not use this file except in compliance with the License", "http://www.apache.org/licenses/LICENSE-2.0",
                 'distributed under the License is distributed on an "AS IS" BASIS', "WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND",
                 "limitations under the License.")
        data = ("/*\n" + "\n".join(parts) + "\n*/").encode()
        self.assertEqual(SUBJECT["original_attribution"](data, "cl.h")["noticeKind"], "apache_short_notice")
        for part in parts[2:]:
            with self.assertRaises(ValueError):
                SUBJECT["original_attribution"](data.replace(part.encode(), b"removed"), "cl.h")

    def test_inline_permission_keeps_full_conditions_and_disclaimer(self):
        parts = ("Copyright 2013 The Khronos Group Inc.", "Permission is hereby granted, free of charge",
                 "distribute, sublicense, and/or sell copies of the Materials", "The above copyright notice and this permission notice shall be included",
                 'THE MATERIALS ARE PROVIDED "AS IS"', "NONINFRINGEMENT", "MATERIALS OR THE USE OR OTHER DEALINGS IN THE MATERIALS.")
        data = ("/*\n" + "\n".join(parts) + "\n*/").encode()
        self.assertEqual(SUBJECT["original_attribution"](data, "gl.h")["noticeKind"], "inline_permission")
        for part in parts[2:]:
            with self.assertRaises(ValueError):
                SUBJECT["original_attribution"](data.replace(part.encode(), b"removed"), "gl.h")

    def test_angle_line_comment_preserves_actual_root_license_reference(self):
        block = ("//\n// Copyright 2017 The ANGLE Project Authors. All rights reserved.\n"
                 "// Use of this source code is governed by a BSD-style license that can be\n// found in the LICENSE file.")
        row = SUBJECT["original_attribution"]((block + "\n#ifndef API\n").encode(), "ext_angle.h")
        self.assertEqual(row["originalAttributionText"], block)
        self.assertEqual(row["noticeKind"], "angle_root_license")
        with self.assertRaises(ValueError):
            SUBJECT["original_attribution"](block.replace("found in the LICENSE file.", "unbound").encode(), "ext_angle.h")

    def test_complete_source_bytes_need_real_regular_path_object_and_digest(self):
        data = b"Copyright original owner\ncomplete license\n"
        path = "third_party/real/LICENSE"
        inputs = {"index": {path: {"mode": "100644", "gitObjectSha1": oid(data)}},
                  "files": {path: {"payload": data, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}}}
        record = SUBJECT["source_license"](inputs, path)
        self.assertEqual(record["sha256"], hashlib.sha256(data).hexdigest())
        for mutation in ("neighbor", "symlink", "digest", "size", "object", "payload"):
            changed = copy.deepcopy(inputs)
            if mutation == "neighbor":
                changed["files"]["third_party/neighbor/LICENSE"] = changed["files"].pop(path)
            elif mutation == "symlink":
                changed["index"][path]["mode"] = "120000"
            elif mutation == "object":
                changed["index"][path]["gitObjectSha1"] = "0" * 40
            elif mutation == "size":
                changed["files"][path]["bytes"] += 1
            elif mutation == "digest":
                changed["files"][path]["sha256"] = "0" * 64
            else:
                changed["files"][path]["payload"] += b" "
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                SUBJECT["source_license"](changed, path)

    def omissions(self):
        mappings = {"mapped/" + str(number): {"license": {"sourcePath": "real/LICENSE"}} for number in range(10)}
        paths = set(mappings) | set(SUBJECT["COPY_SOURCES"]) | SUBJECT["UNRESOLVED"]
        rows = [{"directory": path, "errors": ["original error"]} for path in sorted(paths)]
        headers = [{"sourcePath": SUBJECT["ANGLE"] + prefixes[0] + "api.h"} for prefixes in SUBJECT["COPY_SOURCES"].values()]
        declarations = [{"path": path + "/src", "conditionRecorded": "original condition", "conditionEvaluated": False} for path in SUBJECT["UNRESOLVED"]]
        targets = [{"target": str(number), "manifest": {"src": {"revision": "fixed"}}} for number in range(4)]
        return rows, mappings, headers, declarations, targets

    def test_manifest_absence_does_not_close_original_shipping_obligations(self):
        args = self.omissions()
        before = copy.deepcopy(args)
        report = SUBJECT["omission_records"](*args)
        self.assertEqual(args, before)
        self.assertEqual(len(report), 17)
        self.assertTrue(all(not row["originalErrorRemoved"] and not row["binaryExclusionProven"] for row in report))
        unresolved = [row for row in report if row["disposition"] == "unresolved_source_or_shipping_obligation"]
        self.assertEqual(len(unresolved), 4)
        self.assertTrue(all(not row["manifestAbsenceMeansNotShipped"] for row in unresolved))
        self.assertTrue(all(len(row["recordedManifestEntries"]) == 4 for row in unresolved))

    def test_omitted_or_duplicate_original_record_and_missing_copy_source_fail(self):
        for mutation in ("missing", "duplicate", "copied", "declared"):
            args = list(self.omissions())
            if mutation == "missing":
                args[0].pop()
            elif mutation == "duplicate":
                args[0][-1] = args[0][0]
            elif mutation == "copied":
                args[2].clear()
            else:
                args[3].clear()
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                SUBJECT["omission_records"](*args)

    def test_fragment_retains_copyright_and_complete_text_bytes(self):
        texts = {"Apache-2.0": b"complete APACHE\n", "MIT": b"complete MIT\n"}
        pins = {key: {"oid": oid(data), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()} for key, data in texts.items()}
        headers = [{"sourcePath": "api.h", "originalAttributionText": "/* Copyright original owner */"}]
        with patch.dict(SUBJECT["SPDX"], LICENSES=pins):
            fragment = SUBJECT["render_fragment"](headers, texts, b"ANGLE LICENSE\n")
            self.assertIn(b"NOT FOR DISTRIBUTION", fragment)
            self.assertIn(b"/* Copyright original owner */", fragment)
            for data in texts.values():
                self.assertIn(data, fragment)
            self.assertIn(b"ANGLE LICENSE\n", fragment)
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](headers, {**texts, "MIT": texts["MIT"] + b" "}, b"ANGLE LICENSE\n")
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](headers, {"MIT": texts["MIT"]}, b"ANGLE LICENSE\n")
            with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(fragment)):
                self.assertEqual(SUBJECT["render_fragment"](headers, texts, b"ANGLE LICENSE\n"), fragment)
            with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(fragment) - 1), self.assertRaises(ValueError):
                SUBJECT["render_fragment"](headers, texts, b"ANGLE LICENSE\n")

    def test_existing_or_foreign_output_never_overwrites_original_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-copied-notice-") as name:
            root = Path(name)
            output = root / "notice-supplements-01"
            output.mkdir()
            marker = output / "keep.txt"
            marker.write_bytes(b"original")
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda *_: None):
                for wrong in (root / "foreign", root / "../notice-supplements-01"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](root, wrong)
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](root, output)
            self.assertEqual(marker.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
