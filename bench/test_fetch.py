"""Offline tests for corpus mirror selection and checksum enforcement.

Run with python3 -m unittest discover -s bench -p test_fetch.py.
"""

import contextlib
import gzip
import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import fetch


class FetchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.here = self.root / "bench"
        self.here.mkdir()
        (self.here / "files").mkdir()
        self.out = self.root / "corpus"
        self.out.mkdir()
        self.body = b"pinned benchmark bytes"
        self.doc = {
            "id": "example",
            "file": "example.pdf",
            "sha256": hashlib.sha256(self.body).hexdigest(),
            "url": "https://upstream.invalid/original.pdf",
            "mirror_url": "https://github.com/example/repo/releases/download/corpus-v1/example.pdf",
        }
        self.target = self.out / self.doc["file"]
        self.part = self.out / "example.pdf.part"

    def run_fetch(self):
        (self.here / "corpus.json").write_text(json.dumps({"docs": [self.doc]}))
        with patch.object(fetch, "HERE", self.here), patch.object(
            fetch.sys, "argv", ["fetch.py", str(self.out)]
        ), contextlib.redirect_stdout(io.StringIO()):
            fetch.main()

    def test_verified_cache_needs_no_download(self):
        self.target.write_bytes(self.body)
        with patch.object(fetch, "download") as download:
            self.run_fetch()
        download.assert_not_called()

    def test_committed_file_needs_no_download(self):
        (self.here / "files" / self.doc["file"]).write_bytes(self.body)
        with patch.object(fetch, "download") as download:
            self.run_fetch()
        download.assert_not_called()
        self.assertEqual(self.target.read_bytes(), self.body)

    def test_mirror_download_replaces_corrupt_cache(self):
        self.target.write_bytes(b"bad cache")
        with patch.object(fetch, "download", side_effect=lambda url, path: path.write_bytes(self.body)) as download:
            self.run_fetch()
        download.assert_called_once_with(self.doc["mirror_url"], self.part)
        self.assertEqual(self.target.read_bytes(), self.body)
        self.assertFalse(self.part.exists())

    def test_corrupt_download_is_rejected(self):
        with patch.object(fetch, "download", side_effect=lambda url, path: path.write_bytes(b"bad")):
            with self.assertRaisesRegex(SystemExit, "checksum mismatch"):
                self.run_fetch()
        self.assertFalse(self.target.exists())
        self.assertFalse(self.part.exists())

    def test_corrupt_committed_file_is_rejected(self):
        (self.here / "files" / self.doc["file"]).write_bytes(b"bad")
        with patch.object(fetch, "download") as download:
            with self.assertRaisesRegex(SystemExit, "checksum mismatch"):
                self.run_fetch()
        download.assert_not_called()
        self.assertFalse(self.target.exists())
        self.assertFalse(self.part.exists())

    def test_failed_download_cleans_partial_without_upstream_fallback(self):
        def fail(url, path):
            path.write_bytes(b"partial")
            raise OSError("mirror unavailable")

        self.target.write_bytes(b"old cache")
        with patch.object(fetch, "download", side_effect=fail) as download:
            with self.assertRaisesRegex(SystemExit, "mirror unavailable"):
                self.run_fetch()
        download.assert_called_once_with(self.doc["mirror_url"], self.part)
        self.assertEqual(self.target.read_bytes(), b"old cache")
        self.assertFalse(self.part.exists())

    def test_missing_mirror_fails_without_upstream_fallback(self):
        del self.doc["mirror_url"]
        with patch.object(fetch, "download") as download:
            with self.assertRaises(SystemExit):
                self.run_fetch()
        download.assert_not_called()

    def test_download_uses_default_tls_verification(self):
        response = io.BytesIO(self.body)
        response.headers = {}
        with patch.object(fetch.urllib.request, "urlopen", return_value=response) as urlopen:
            fetch.download(self.doc["mirror_url"], self.part)
        self.assertEqual(self.part.read_bytes(), self.body)
        self.assertEqual(urlopen.call_args.args[0].full_url, self.doc["mirror_url"])
        self.assertEqual(urlopen.call_args.kwargs, {"timeout": 120})

    def test_gzip_transfer_decodes_before_hashing(self):
        response = io.BytesIO(gzip.compress(self.body))
        response.headers = {"Content-Encoding": "gzip"}
        with patch.object(fetch.urllib.request, "urlopen", return_value=response):
            fetch.download(self.doc["mirror_url"], self.part)
        self.assertEqual(fetch.sha256(self.part), self.doc["sha256"])

    def test_transport_retries_are_bounded(self):
        with patch.object(fetch.urllib.request, "urlopen", side_effect=OSError("offline")) as urlopen, patch.object(
            fetch.time, "sleep"
        ) as sleep, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaisesRegex(OSError, "offline"):
                fetch.download(self.doc["mirror_url"], self.part)
        self.assertEqual(urlopen.call_count, 5)
        self.assertEqual(sleep.call_count, 4)
        self.assertFalse(self.part.exists())


class ManifestTests(unittest.TestCase):
    def test_all_mirrors_are_pinned_release_assets(self):
        corpus = json.loads((fetch.HERE / "corpus.json").read_text())
        base = "https://github.com/SylphxAI/anymd/releases/download/agentdocbench-corpus-v1/"
        self.assertEqual(len(corpus["docs"]), 38)
        for doc in corpus["docs"]:
            with self.subTest(doc=doc["id"]):
                self.assertEqual(doc["mirror_url"], base + doc["file"])
                self.assertNotEqual(doc["url"], doc["mirror_url"])
                self.assertEqual(len(doc["sha256"]), 64)
                self.assertTrue(doc["license_evidence"])
                self.assertTrue(doc["attribution"])


if __name__ == "__main__":
    unittest.main()
