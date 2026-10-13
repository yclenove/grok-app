"""Actual manifest/config identity is not binary shipping or license exemption."""
import json
from pathlib import Path
import runpy
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-archive-checkouts.py")))


class ArchiveCheckoutTests(unittest.TestCase):
    def manifest(self):
        return {"did_run": True, "root": "src", "fixed_revisions": {"src": SUBJECT["INPUTS"]["TREE"]["COMMIT"]},
                "manifest": {"src": {"repository": "https://chromium.googlesource.com/chromium/src.git",
                                      "revision": SUBJECT["INPUTS"]["TREE"]["COMMIT"]}}}

    def bootstrap(self, target="linux-x64"):
        builder = SUBJECT["BUILDS"]["archive"](target)[0]
        commit = {"host": "chromium.googlesource.com", "project": "chromium/src", "id": SUBJECT["INPUTS"]["TREE"]["COMMIT"]}
        return {"recipe": "chromium", "$build/chromium_bootstrap": {
            "commits": [commit], "config_source": {"path": "infra/config/generated/builders/ci/" + builder + "/properties.json",
                                                   "last_changed_commit": {**commit, "id": SUBJECT["CONFIG_REVISION"]}},
            "exe": {"cipd": {"server": "https://chrome-infra-packages.appspot.com",
                             "package": "infra/recipe_bundles/chromium.googlesource.com/chromium/tools/build",
                             "actual_version": SUBJECT["RECIPE_INSTANCE"], "requested_version": "refs/heads/main"},
                    "cmd": ["luciexe"]}}, "$build/chromium_tests_builder_config": {"builder_config": {"chromium_config": "chromium"}}}

    def test_actual_manifest_exact_root_source_and_checkout_paths(self):
        result = SUBJECT["manifest_data"](json.dumps(self.manifest()).encode())
        self.assertEqual(len(result), 1)
        for key, value in (("did_run", False), ("root", "other"), ("fixed_revisions", {"src": "a" * 40})):
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["manifest_data"](json.dumps({**self.manifest(), key: value}).encode())

    def test_foreign_repository_credentials_revision_or_escaping_manifest_reject(self):
        for path, repository, revision in (("src/../escape", "https://example.test/source", "a" * 40),
                                           ("src/other", "https://user:secret@example.test/source", "a" * 40),
                                           ("src/other", "file:///source", "a" * 40),
                                           ("src/other", "https://example.test/source", "other")):
            data = self.manifest()
            data["manifest"][path] = {"repository": repository, "revision": revision}
            with self.subTest(path=path, revision=revision), self.assertRaises(ValueError):
                SUBJECT["manifest_data"](json.dumps(data).encode())

    def test_resolved_recipe_instance_and_config_revision_are_original_not_latest_alias(self):
        for target in SUBJECT["BUILDS"]["ARCHIVES"]:
            result = SUBJECT["bootstrap_data"](target, json.dumps(self.bootstrap(target)).encode())
            self.assertTrue(result["resolvedInstanceRecorded"])
            self.assertFalse(result["recipeSourceCollected"])
            self.assertFalse(result["actualGclientHelperRevisionVerified"])
            self.assertEqual(result["recipeExe"]["cipd"]["requested_version"], "refs/heads/main")
        data = self.bootstrap()
        data["$build/chromium_bootstrap"]["exe"]["cipd"]["actual_version"] = "refs/heads/main"
        with self.assertRaises(ValueError):
            SUBJECT["bootstrap_data"]("linux-x64", json.dumps(data).encode())

    def test_cross_builder_source_config_and_recipe_command_reject(self):
        for variant in ("config", "source", "command", "container"):
            data = self.bootstrap()
            bootstrap = data["$build/chromium_bootstrap"]
            if variant == "config":
                bootstrap["config_source"]["path"] = "infra/config/generated/builders/ci/win-archive-rel/properties.json"
            elif variant == "source":
                bootstrap["commits"][0]["id"] = "b" * 40
            elif variant == "container":
                data["$build/chromium_tests_builder_config"] = []
            else:
                bootstrap["exe"]["cmd"] = ["other"]
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["bootstrap_data"]("linux-x64", json.dumps(data).encode())

    def test_match_absent_and_different_do_not_make_shipping_decisions_or_url_aliases(self):
        manifest = {"src/known": {"repository": "https://example.test/known.git", "revision": "a" * 40}}
        for repository, status in (("https://example.test/known.git", "exact_repository_and_revision_match"),
                                   ("https://example.test/known", "manifest_identity_differs")):
            result = SUBJECT["compare_pin"]("src/known", repository, "a" * 40, manifest)
            self.assertEqual(result["status"], status)
            self.assertFalse(result["shippingDecisionMade"])
        result = SUBJECT["compare_pin"]("src/absent", "https://example.test/absent", "b" * 40, manifest)
        self.assertEqual(result["status"], "exact_manifest_entry_absent")
        self.assertFalse(result["shippingDecisionMade"])
        collected = SUBJECT["source_comparison"]({})
        self.assertEqual(len(collected), 57)
        self.assertTrue(all(row["status"] == "exact_manifest_entry_absent" and not row["shippingDecisionMade"] for row in collected))

    def test_recursive_conditions_are_recorded_never_evaluated_or_aliased_to_parent(self):
        catalog = {"parentSourceCommit": SUBJECT["INPUTS"]["TREE"]["COMMIT"], "dependencyComponent": "angle",
                   "relativeLayoutDeclared": True, "parentCheckoutPath": "third_party/angle", "gitDependencies": [
                       {"path": "third_party/child", "repository": "https://example.test/child", "revision": "a" * 40,
                        "condition": "not build_with_chromium"}]}
        rows = SUBJECT["recursive_comparison"]({}, catalog)
        self.assertEqual(rows[0]["checkoutPath"], "src/third_party/angle/third_party/child")
        self.assertFalse(rows[0]["conditionEvaluated"])
        self.assertFalse(rows[0]["shippingDecisionMade"])
        self.assertEqual(rows[0]["conditionRecorded"], "not build_with_chromium")
        catalog["gitDependencies"][0]["path"] = "../child"
        with self.assertRaises(ValueError):
            SUBJECT["recursive_comparison"]({}, catalog)

    def test_invalid_recursive_catalog_and_counts_keep_discrepancies_visible(self):
        with self.assertRaises(ValueError):
            SUBJECT["recursive_comparison"]({}, {"parentSourceCommit": "other"})
        rows = [{"status": "exact_repository_and_revision_match"}, {"status": "manifest_identity_differs"},
                {"status": "exact_manifest_entry_absent"}]
        self.assertEqual(SUBJECT["status_counts"](rows), {"exact_repository_and_revision_match": 1,
                                                        "manifest_identity_differs": 1, "exact_manifest_entry_absent": 1})


if __name__ == "__main__":
    unittest.main()
