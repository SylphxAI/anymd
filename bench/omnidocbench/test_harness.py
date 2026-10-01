"""Offline tests for full-corpus selection and the evaluator's ground-truth CLI."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('harness', Path(__file__).with_name('harness.py'))
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)


source_spec = importlib.util.spec_from_file_location('verify_source', Path(__file__).with_name('verify_source.py'))
source = importlib.util.module_from_spec(source_spec)
source_spec.loader.exec_module(source)


class SourceTests(unittest.TestCase):
    def setUp(self):
        self.sha = 'a' * 40
        self.run = {'id': 123, 'head_sha': self.sha, 'path': '.github/workflows/omnidocbench.yml'}
        self.jobs = [{'name': f'Pages {i}/32', 'status': 'completed', 'conclusion': 'success'} for i in range(1, 33)]
        self.log = '2026-10-01T00:00:00Z   ANYMD_OCR: vlm\n2026-10-01T00:00:00Z   LIMIT: 0\n'

    def test_source_provenance_matches(self):
        self.assertEqual(source.validate(self.run, self.jobs, self.log, self.sha, 'vlm', 0)['source_sha'], self.sha)

    def test_source_mismatches_fail_closed(self):
        for sha, engine, limit in [('b' * 40, 'vlm', 0), (self.sha, 'tesseract', 0), (self.sha, 'vlm', 40)]:
            with self.assertRaises(ValueError):
                source.validate(self.run, self.jobs, self.log, sha, engine, limit)

    def test_incomplete_shard_fails_closed(self):
        self.jobs[0]['status'] = 'in_progress'
        with self.assertRaises(ValueError):
            source.validate(self.run, self.jobs, self.log, self.sha, 'vlm', 0)


class HarnessTests(unittest.TestCase):
    def setUp(self):
        self.pages = [{'page_info': {'image_path': f'{i:04d}.png'}} for i in range(1651)]

    def test_full_corpus_shards_cover_every_page_once(self):
        names = harness.page_names(self.pages, 0)
        shards = [names[i::32] for i in range(32)]
        self.assertEqual(sorted(map(len, shards)), [51] * 13 + [52] * 19)
        self.assertEqual(len({name for shard in shards for name in shard}), 1651)

    def test_partial_selection_is_explicit(self):
        self.assertEqual(len(harness.page_names(self.pages, 40)), 40)

    def test_ground_truth_cli_writes_full_and_partial_counts(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / 'gt.json'
            for limit, count in [(0, 1651), (40, 40)]:
                with patch.object(sys, 'argv', ['harness.py', 'gt', '--limit', str(limit),
                                               '--out', str(output)]), \
                        patch.object(harness, 'annotations', return_value=(None, self.pages)), \
                        patch('builtins.print'):
                    harness.main()
                self.assertEqual(len(json.loads(output.read_text())), count)


if __name__ == '__main__':
    unittest.main()
