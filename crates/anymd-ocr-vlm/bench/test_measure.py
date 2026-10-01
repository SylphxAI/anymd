"""Measurement reporting tests; no models, Rust build or psutil required."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('measure', Path(__file__).with_name('measure.py'))
measure = importlib.util.module_from_spec(spec)
with patch.dict(sys.modules, {'psutil': types.SimpleNamespace(Process=lambda pid: None)}):
    spec.loader.exec_module(measure)


class MeasurementTests(unittest.TestCase):
    def run_measurement(self, payload, exit_code=0, error=''):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            binary = root / 'anymd'
            binary.write_text('placeholder')
            (root / 'manifest.jsonl').write_text('\n'.join(
                json.dumps({'file': f'{i}.png', 'text': 'reference'}) for i in range(50)))
            output = root / 'measure.json'

            def launch(args, stdout, stderr):
                stdout.write(payload)
                stderr.write(error)
                return types.SimpleNamespace(pid=123, returncode=exit_code, poll=lambda: exit_code)

            with patch.object(sys, 'argv', ['measure.py', '--binary', str(binary),
                                           '--pages', str(root), '--out', str(output)]), \
                    patch.object(measure.subprocess, 'Popen', launch), \
                    patch.object(measure.subprocess, 'check_output', return_value='anymd 8.3.0'), \
                    patch.dict(os.environ, {'ANYMD_OCR_QUANTIZATION': 'q4'}), \
                    patch('builtins.print'):
                try:
                    measure.main()
                    failed = False
                except SystemExit:
                    failed = True
            return json.loads(output.read_text()), failed

    def test_success_has_real_quality_and_mode(self):
        result, failed = self.run_measurement(json.dumps({'text': 'reference', 'quantization': 'q4'}))
        self.assertFalse(failed)
        self.assertEqual(result['status'], 'measured')
        self.assertEqual(result['requested_quantization'], 'q4')
        self.assertEqual(result['cer_mean'], 0)
        self.assertEqual(result['pages_ok'], 10)
        self.assertIsNotNone(result['median_seconds'])

    def test_unsupported_mode_never_becomes_a_quality_measurement(self):
        result, failed = self.run_measurement('', 1, 'unsupported quantized tensor')
        self.assertTrue(failed)
        self.assertEqual(result['status'], 'failed')
        for field in ['median_seconds', 'p90_seconds', 'peak_rss_mib', 'cer_mean']:
            self.assertIsNone(result[field])
        self.assertEqual(result['pages_ok'], 0)
        self.assertEqual(result['pages'][0]['error'], 'unsupported quantized tensor')
        self.assertIsNone(result['pages'][0]['cer'])

    def test_malformed_successful_worker_is_still_failure(self):
        result, failed = self.run_measurement('{}')
        self.assertTrue(failed)
        self.assertEqual(result['pages_ok'], 0)
        self.assertIsNone(result['cer_mean'])


if __name__ == '__main__':
    unittest.main()
