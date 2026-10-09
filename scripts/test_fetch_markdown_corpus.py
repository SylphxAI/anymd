#!/usr/bin/env python3
"""Guard corpus download retries and checksum promotion without a network."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("fetch-markdown-corpus.sh")
CONTENT = b"pinned corpus fixture\n"


class CorpusDownloadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.tools = root / "tools"
        self.tools.mkdir()
        self.downloads = root / "downloads"
        self.downloads.mkdir()
        self.calls = root / "calls"
        self.env = dict(os.environ, PATH=f"{self.tools}:{os.environ['PATH']}",
                        CALLS=str(self.calls), PAYLOAD=CONTENT.decode(),
                        SHA=hashlib.sha256(CONTENT).hexdigest(), CURL_EXIT="0")
        self.tool("jq", 'import os\nprint("fixture.pdf\\thttps://example.invalid/fixture.pdf\\t" + os.environ["SHA"])\n')
        self.tool("curl", '''import os, pathlib, sys
args = sys.argv[1:]
pathlib.Path(os.environ["CALLS"]).write_text("\\n".join(args))
pathlib.Path(args[args.index("-o") + 1]).write_text(os.environ["PAYLOAD"])
sys.exit(int(os.environ["CURL_EXIT"]))
''')

    def tool(self, name, source):
        path = self.tools / name
        path.write_text(f"#!{sys.executable}\n" + source)
        path.chmod(0o700)

    def fetch(self):
        return subprocess.run(["bash", str(SCRIPT), str(self.downloads)],
                              env=self.env, capture_output=True, text=True, timeout=10)

    def test_download_retries_transport_errors_with_time_bounds(self):
        result = self.fetch()
        self.assertEqual(result.returncode, 0, result.stderr)
        args = self.calls.read_text().splitlines()
        self.assertIn("--retry-all-errors", args)
        for flag, value in (("--retry", "3"), ("--retry-max-time", "120"),
                            ("--connect-timeout", "15"), ("--max-time", "60")):
            self.assertEqual(args[args.index(flag) + 1], value)
        self.assertIn("--fail", args)
        self.assertEqual((self.downloads / "fixture.pdf").read_bytes(), CONTENT)
        self.assertFalse((self.downloads / "fixture.pdf.tmp").exists())

    def test_exhausted_tls_failure_is_not_promoted(self):
        self.env["CURL_EXIT"] = "35"
        self.assertEqual(self.fetch().returncode, 35)
        self.assertFalse((self.downloads / "fixture.pdf").exists())

    def test_checksum_failure_is_not_promoted(self):
        self.env["PAYLOAD"] = "wrong bytes"
        result = self.fetch()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse((self.downloads / "fixture.pdf").exists())
        self.assertFalse((self.downloads / "fixture.pdf.tmp").exists())

    def test_verified_cache_skips_download(self):
        (self.downloads / "fixture.pdf").write_bytes(CONTENT)
        self.assertEqual(self.fetch().returncode, 0)
        self.assertFalse(self.calls.exists())

    def test_corrupt_cache_is_replaced_after_verification(self):
        (self.downloads / "fixture.pdf").write_bytes(b"corrupt")
        self.assertEqual(self.fetch().returncode, 0)
        self.assertEqual((self.downloads / "fixture.pdf").read_bytes(), CONTENT)
        self.assertTrue(self.calls.exists())


if __name__ == "__main__":
    unittest.main()
