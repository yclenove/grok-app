"""Offline contracts for data-only provenance collection; no binary proof."""
import hashlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch


SPEC = importlib.util.spec_from_file_location(
    "provenance", Path(__file__).with_name("inspect-ungoogled-provenance.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)


def blob(path="README.md", content=b"source, not executed"):
    return {"path": path, "mode": "100644", "type": "blob", "size": len(content),
            "sha": hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()}


class ProvenanceTests(unittest.TestCase):
    def test_regular_executable_mode_and_gitlink_are_metadata_only(self):
        entries = [blob(), dict(blob("build.py"), mode="100755"),
                   {"path": "patchset", "mode": "160000", "type": "commit", "sha": "b" * 40}]
        self.assertEqual(SUBJECT.tree_records({"truncated": False, "tree": entries}), entries)

    def test_truncated_empty_and_oversized_tree_reject(self):
        for document in ({"truncated": True, "tree": [blob()]}, {"truncated": False, "tree": []},
                         {"truncated": False, "tree": [blob()] * (SUBJECT.MAX_ENTRIES + 1)}):
            with self.assertRaises(ValueError):
                SUBJECT.tree_records(document)

    def test_ambiguous_paths_links_and_hashes_reject(self):
        for row in (blob("../outside"), blob("C:outside"), blob("a\\b"), blob("/root"),
                    blob("a\0b"), blob("a//b"), dict(blob(), mode="120000"),
                    dict(blob(), sha="not-a-hash")):
            with self.subTest(row=row), self.assertRaises(ValueError):
                SUBJECT.tree_records({"truncated": False, "tree": [row]})

    def test_case_duplicates_reject(self):
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            SUBJECT.tree_records({"truncated": False, "tree": [blob(), blob("readme.MD")]})

    def test_size_and_total_budgets_are_not_tree_claims(self):
        for size in (-1, True, SUBJECT.MAX_BLOB + 1):
            with self.subTest(size=size), self.assertRaises(ValueError):
                SUBJECT.tree_records({"truncated": False, "tree": [dict(blob(), size=size)]})
        with patch.object(SUBJECT, "MAX_REPOSITORY", 1), self.assertRaisesRegex(ValueError, "total"):
            SUBJECT.tree_records({"truncated": False, "tree": [blob()]})

    def test_actual_blob_identity_and_bytes_required(self):
        content = b"source, not executed"
        with patch.object(SUBJECT, "fetch", return_value=content):
            record, result = SUBJECT.blob_record("repo", "a" * 40, blob())
        self.assertEqual(result, content)
        self.assertEqual(record["sha256"], hashlib.sha256(content).hexdigest())
        for changed in (b"x" * len(content), content[:-1]):
            with patch.object(SUBJECT, "fetch", return_value=changed), self.assertRaisesRegex(ValueError, "identity"):
                SUBJECT.blob_record("repo", "a" * 40, blob())

    def test_credentials_http_and_unknown_hosts_reject_before_network(self):
        for url in ("http://api.github.com/x", "https://example.com/x",
                    "https://secret@api.github.com/x", "https://api.github.com.evil/x"):
            with patch.object(SUBJECT.urllib.request, "urlopen") as network, self.assertRaises(ValueError):
                SUBJECT.fetch(url, 10)
            network.assert_not_called()

    def test_transient_read_only_timeout_retries_are_bounded(self):
        url = "https://raw.githubusercontent.com/owner/repo/fixed/file"
        response = MagicMock()
        response.url, response.status = url, 200
        response.read.return_value = b"ok"
        response.__enter__.return_value = response
        with patch.object(SUBJECT.urllib.request, "urlopen", side_effect=[
                TimeoutError(), TimeoutError(), response]) as network:
            self.assertEqual(SUBJECT.fetch(url, 2), b"ok")
            self.assertEqual(network.call_count, 3)
        with patch.object(SUBJECT.urllib.request, "urlopen", side_effect=TimeoutError()) as network, \
                self.assertRaises(TimeoutError):
            SUBJECT.fetch(url, 2)
        self.assertEqual(network.call_count, 3)

    def test_redirect_and_oversized_response_do_not_retry(self):
        url = "https://raw.githubusercontent.com/owner/repo/fixed/file"
        for final_url, content in (("https://example.com/redirect", b"ok"), (url, b"too large")):
            response = MagicMock()
            response.url, response.status = final_url, 200
            response.read.return_value = content
            response.__enter__.return_value = response
            with patch.object(SUBJECT.urllib.request, "urlopen", return_value=response) as network, \
                    self.assertRaises(ValueError):
                SUBJECT.fetch(url, 2)
            self.assertEqual(network.call_count, 1)

    def test_not_found_is_not_retried(self):
        url = "https://raw.githubusercontent.com/owner/repo/fixed/file"
        error = SUBJECT.urllib.error.HTTPError(url, 404, "not found", None, io.BytesIO(b"not found"))
        with patch.object(SUBJECT.urllib.request, "urlopen", side_effect=error) as network, \
                self.assertRaises(SUBJECT.urllib.error.HTTPError):
            SUBJECT.fetch(url, 2)
        self.assertEqual(network.call_count, 1)

    def test_output_must_be_new_direct_run_child(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-source-output-oracle-") as temporary:
            root = Path(temporary)
            (root / ".run").mkdir()
            with patch.object(SUBJECT, "HERE", root):
                with self.assertRaises(ValueError):
                    SUBJECT.new_evidence_directory(root / "outside")
                output = SUBJECT.new_evidence_directory(root / ".run" / "new")
                self.assertEqual(output, root / ".run" / "new")
                with self.assertRaises(FileExistsError):
                    SUBJECT.new_evidence_directory(output)

    def test_source_paths_never_create_executable_script_or_nested_local_path(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-data-only-oracle-") as temporary:
            output = Path(temporary)
            item = {"path": "scripts/build.py", "sha256": hashlib.sha256(b"data").hexdigest()}
            SUBJECT.save_repository(output, 0, [(item, b"data")], b"tree metadata")
            evidence = output / item["evidence"]
            self.assertEqual(evidence.suffix, ".source-data")
            self.assertEqual(evidence.read_bytes(), b"data")
            self.assertFalse((output / "scripts").exists())
            with self.assertRaises(FileExistsError):
                SUBJECT.save_repository(output, 0, [(item, b"data")], b"tree metadata")

    def test_terminal_failure_cancels_queue_and_exits_original_pool(self):
        commit, tree_sha = "a" * 40, "b" * 40
        metadata = {"sha": commit, "tree": {"sha": tree_sha}}
        tree = {"sha": tree_sha, "truncated": False, "tree": [blob(), blob("second")]}
        executor, failed, pending = MagicMock(), MagicMock(), MagicMock()
        executor.__enter__.return_value = executor
        executor.submit.side_effect = [failed, pending]
        failed.result.side_effect = ValueError("identity mismatch")
        with patch.object(SUBJECT, "fetch", side_effect=[json.dumps(metadata).encode(),
                                                       json.dumps(tree).encode()]), \
                patch.object(SUBJECT.concurrent.futures, "ThreadPoolExecutor", return_value=executor), \
                self.assertRaisesRegex(ValueError, "identity"):
            SUBJECT.collect_repository("repo", commit)
        failed.cancel.assert_called_once()
        pending.cancel.assert_called_once()
        executor.__exit__.assert_called_once()

    def test_unexpected_commit_and_tree_reject(self):
        commit = "a" * 40
        for metadata in ({"sha": "b" * 40, "tree": {"sha": "c" * 40}},
                         {"sha": commit, "tree": {"sha": "invalid"}}):
            with patch.object(SUBJECT, "fetch", return_value=json.dumps(metadata).encode()), \
                    self.assertRaisesRegex(ValueError, "identity"):
                SUBJECT.collect_repository("repo", commit)
        metadata = {"sha": commit, "tree": {"sha": "b" * 40}}
        wrong_tree = {"sha": "c" * 40, "truncated": False, "tree": [blob()]}
        with patch.object(SUBJECT, "fetch", side_effect=[json.dumps(metadata).encode(),
                                                       json.dumps(wrong_tree).encode()]), \
                self.assertRaisesRegex(ValueError, "resolved tree"):
            SUBJECT.collect_repository("repo", commit)

    def test_data_only_sources_do_not_claim_approval(self):
        commit, tree_sha = "a" * 40, "b" * 40
        content = b"source, not executed"
        metadata = {"sha": commit, "tree": {"sha": tree_sha}}
        tree = {"sha": tree_sha, "truncated": False, "tree": [blob()]}
        with patch.object(SUBJECT, "fetch", side_effect=[json.dumps(metadata).encode(),
                                                       json.dumps(tree).encode(), content]):
            record, data, _ = SUBJECT.collect_repository("repo", commit)
        self.assertEqual(record["sourceFiles"], 1)
        self.assertEqual(record["sourceBytes"], len(content))
        self.assertEqual(data[0][1], content)
        self.assertNotIn("runtimePrepared", record)


if __name__ == "__main__":
    unittest.main()
