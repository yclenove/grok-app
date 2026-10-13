"""Proposal contract oracles; these do not certify or execute browser binaries."""
import importlib.util
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import Mock, patch


SPEC = importlib.util.spec_from_file_location(
    "proposal_audit", Path(__file__).with_name("inspect-ungoogled-chromium-sources.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)


class ProposalTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="grok-cu-source-contract-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.path = self.directory / "Win_x64.zip"
        with zipfile.ZipFile(self.path, "w") as archive:
            for name in ("chrome.exe", "chrome.dll", "chrome_elf.dll"):
                archive.writestr(f"{SUBJECT.ROOT}/{name}", b"oracle, not executable")

    def inspect(self, kind="full_candidate", tree_sha=None, architecture="pe-x86_64", size=None):
        credits = {"creditsCandidates": [{"documentKind": kind}], "packs": []}
        with patch.dict(SUBJECT.CREDITS, {"inspect_pinned_archive": Mock(return_value=(credits, [b"oracle"]))}), \
                patch.dict(SUBJECT.SOURCE, {"inspect": Mock(return_value={
                    "archiveSha256": tree_sha or SUBJECT.SHA256})}), \
                patch.dict(SUBJECT.NATIVE, {"native_header": Mock(return_value={
                    "formatArchitecture": architecture})}), \
                patch.object(SUBJECT, "ARCHIVE_BYTES", self.path.stat().st_size if size is None else size):
            return SUBJECT.inspect_directory(self.directory)

    def test_credits_candidate_never_claims_redistribution_native_or_other_platform_acceptance(self):
        report, payloads = self.inspect()
        self.assertEqual(report["embeddedFullNoticesGate"], "candidate_only_not_certified")
        self.assertEqual(report["redistributionGate"],
                         "not_proven_codec_and_exact_patched_source_review_required")
        self.assertEqual(report["nativeAcceptance"], "not_run")
        self.assertEqual(report["securityAcceptance"], "not_audited")
        self.assertEqual(report["otherTargets"],
                         {target: "not_audited" for target in ("Linux_x64", "Mac_Arm", "Mac")})
        self.assertEqual((report["executed"], report["promoted"]), (False, False))
        self.assertEqual(payloads, [[b"oracle"]])

    def test_sample_and_unproven_document_remain_red(self):
        for kind in ("sample_stub", "unproven_document"):
            with self.subTest(kind=kind):
                report, _ = self.inspect(kind=kind)
                self.assertEqual(report["embeddedFullNoticesGate"], "failed_missing_full_notices")
                self.assertFalse(report["promoted"])

    def test_source_size_mismatch_is_not_blessed_by_credits(self):
        with self.assertRaisesRegex(ValueError, "size"):
            self.inspect(size=self.path.stat().st_size + 1)

    def test_changed_source_hash_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "changed"):
            self.inspect(tree_sha="b" * 64)

    def test_wrong_native_architecture_is_rejected_without_execution(self):
        with self.assertRaisesRegex(ValueError, "architecture"):
            self.inspect(architecture="pe-arm64")

    def test_original_pin_failure_stops_before_tree_inventory(self):
        inventory = Mock()
        with patch.dict(SUBJECT.SOURCE, {"inspect": inventory}), self.assertRaisesRegex(ValueError, "SHA256"):
            SUBJECT.inspect_directory(self.directory)
        inventory.assert_not_called()

    def test_source_provenance_does_not_claim_archive_download_or_reproduction(self):
        report, _ = self.inspect()
        self.assertEqual(report["sourceArchive"]["sha256"],
                         "d450182ea38a6485febfdfdad6f3a3f1d5d3408cc368329b94502c629ef8ec7e")
        self.assertFalse(report["sourceArchive"]["downloaded"])
        self.assertFalse(report["sourceArchive"]["reproduced"])
        self.assertTrue(report["publisherBuildFlags"]["proprietaryCodecs"])
        self.assertFalse(report["publisherBuildFlags"]["enableSwiftshader"])


if __name__ == "__main__":
    unittest.main()
