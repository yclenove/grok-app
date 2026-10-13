"""Original build log literals/defaults never evaluate upstream GN or DEPS."""
import json
from pathlib import Path
import runpy
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-archive-inputs.py")))


class ArchiveInputTests(unittest.TestCase):
    def args(self, body='target_cpu = "x64"\ntarget_os = "linux"', tail=True):
        text = '\nWriting """\\\n' + body + '\n""" to /fixture/out/args.gn.\n\n/fixture/gn gen /fixture/out --check\n'
        if tail:
            text += 'Done. Made 10 targets from 5 files in 2ms\n'
        return text.encode()

    def config(self, custom=None):
        return json.dumps({"solutions": [{"name": "src", "url": "https://chromium.googlesource.com/chromium/src.git",
                                          "deps_file": ".DEPS.git", "custom_vars": {} if custom is None else custom,
                                          "custom_deps": {}}], "target_os": [], "target_cpu": []}).encode()

    def test_four_actual_target_literals_and_crlf_no_platform_aliases(self):
        for target, (os_name, cpu) in SUBJECT["PLATFORMS"].items():
            data = self.args('target_cpu = "' + cpu + '"\ntarget_os = "' + os_name + '"')
            for contents in (data, data.replace(b"\n", b"\r\n")):
                values = SUBJECT["gn_arguments"](contents, target)
                self.assertEqual((values["target_os"], values["target_cpu"]), (os_name, cpu))

    def test_false_true_numeric_values_not_truthy_string_coercion(self):
        data = self.args('target_cpu = "x64"\ntarget_os = "linux"\nis_official_build = false\ngenerate_about_credits = true\nsymbol_level = 0')
        values = SUBJECT["gn_arguments"](data, "linux-x64")
        self.assertIs(values["is_official_build"], False)
        self.assertIs(values["generate_about_credits"], True)
        self.assertEqual(values["symbol_level"], 0)

    def test_duplicate_dynamic_expression_import_and_interpolation_reject_not_execute(self):
        for line in ('target_os = "linux"', 'other = exec("never")', 'import("//dynamic.gni")',
                     'other = "${dynamic}"', 'other = false # comment'):
            with self.subTest(line=line), self.assertRaises(ValueError):
                SUBJECT["gn_arguments"](self.args('target_cpu = "x64"\ntarget_os = "linux"\n' + line), "linux-x64")

    def test_missing_or_duplicated_writing_block_failure_and_wrong_platform_reject(self):
        for data in (b"raw log", self.args() + self.args(), self.args(tail=False), self.args('target_os = "linux"'),
                     self.args('target_os = "mac"\ntarget_cpu = "arm64"')):
            with self.subTest(data=data[:40]), self.assertRaises(ValueError):
                SUBJECT["gn_arguments"](data, "linux-x64")

    def test_actual_custom_vars_kept_as_original_data_not_eval_or_boolean_normalized(self):
        custom = {"download_remoteexec_cfg": "True", "rbe_instance": "public-instance"}
        result = SUBJECT["gclient_custom_vars"](self.config(custom))
        self.assertEqual(result["customVars"], custom)
        self.assertEqual(result["depsFileRecorded"], ".DEPS.git")
        self.assertFalse(result["checkoutManifestVerified"])
        self.assertFalse(result["effectiveInheritedVarsVerified"])

    def test_foreign_or_multiple_solutions_nonobject_and_oversize_custom_data_reject(self):
        for config in ([], {"solutions": []}, {"solutions": [None]},
                       {"solutions": [{"name": "src", "url": "https://other.example/source", "custom_vars": {}, "custom_deps": {}}]}):
            with self.subTest(config=config), self.assertRaises(ValueError):
                SUBJECT["gclient_custom_vars"](json.dumps(config).encode())
        with self.assertRaises(ValueError):
            SUBJECT["gclient_custom_vars"](self.config({str(index): "value" for index in range(129)}))


if __name__ == "__main__":
    unittest.main()
