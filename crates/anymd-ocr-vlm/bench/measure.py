#!/usr/bin/env python3
"""Measure the production CLI worker, including model load and process overhead."""
import argparse
import json
import math
import os
import re
from pathlib import Path
import statistics
import subprocess
import time
import psutil


def normalise(text):
    text = re.sub(r'<[^>]*>', '', text).translate(str.maketrans({'‘': "'", '’': "'", '“': '"', '”': '"'}))
    return ''.join(c for c in text if not c.isspace() and c not in '#*|`_\u200b')


def cer(reference, hypothesis):
    a, b = normalise(reference), normalise(hypothesis)
    previous = list(range(len(b) + 1))
    for i, ca in enumerate(a):
        current = [i + 1]
        for j, cb in enumerate(b):
            current.append(min(previous[j + 1] + 1, current[-1] + 1, previous[j] + (ca != cb)))
        previous = current
    return previous[-1] / max(1, len(a))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    parser.add_argument('--pages', required=True)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    pages = Path(args.pages)
    rows = [json.loads(line) for line in (pages / 'manifest.jsonl').read_text(encoding='utf-8').splitlines()]
    # One clean, scan and photo page per language, plus a mixed page.
    selected = [rows[i] for i in [0, 12, 22, 30, 33, 36, 39, 42, 45, 47]]
    results = []
    for row in selected:
        started = time.monotonic()
        peak = 0
        with open(Path(args.out).with_suffix('.stdout'), 'w+', encoding='utf-8') as output, \
                open(Path(args.out).with_suffix('.stderr'), 'w+', encoding='utf-8') as errors:
            process = subprocess.Popen([args.binary, '__ocr-vlm-worker', str(pages / 'pages' / row['file']), '4096'], stdout=output, stderr=errors)
            tracked = psutil.Process(process.pid)
            while process.poll() is None:
                try:
                    memory = tracked.memory_info()
                    peak = max(peak, getattr(memory, "peak_wset", memory.rss))
                except psutil.NoSuchProcess:
                    pass
                try:
                    process.wait(timeout=0.1)
                except subprocess.TimeoutExpired:
                    if time.monotonic() - started > 600:
                        process.kill()
                        process.wait()
            output.seek(0)
            raw = output.read()
            errors.seek(0)
            error = errors.read()[-8192:]
        evidence = {}
        try:
            evidence = json.loads(raw)
            text = evidence['text']
        except (ValueError, KeyError):
            text = ''
        ok = process.returncode == 0 and isinstance(evidence.get('text'), str)
        results.append({'file': row['file'], 'ok': ok, 'exit_code': process.returncode,
                        'error': error if not ok else None, 'seconds': time.monotonic() - started,
                        'peak_rss_mib': peak / 1048576, 'cer': cer(row['text'], text) if ok else None,
                        'stopped_regions': evidence.get('truncated', 0),
                        'device': evidence.get('device'), 'model_revision': evidence.get('model_revision'),
                        'quantization': evidence.get('quantization')})
    complete = all(r['ok'] for r in results)
    summary = {'pages': results, 'status': 'measured' if complete else 'failed',
               'requested_quantization': os.environ.get('ANYMD_OCR_QUANTIZATION', 'none'),
               'median_seconds': statistics.median(r['seconds'] for r in results) if complete else None,
               'p90_seconds': sorted(r['seconds'] for r in results)[math.ceil(0.9 * len(results)) - 1] if complete else None,
               'head_sha': os.environ.get('GITHUB_SHA'),
               'peak_rss_mib': max(r['peak_rss_mib'] for r in results) if complete else None,
               'cer_mean': statistics.mean(r['cer'] for r in results) if complete else None,
               'pages_ok': sum(r['ok'] for r in results), 'binary_bytes': Path(args.binary).stat().st_size,
               'includes_model_load': True, 'rss_sampling_interval_ms': 100,
               'anymd_version': subprocess.check_output([args.binary, 'version'], text=True).strip()}
    build_info = Path('docvlm-build.json')
    if build_info.is_file():
        summary.update(json.loads(build_info.read_text(encoding='utf-8')))
    Path(args.out).write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary, indent=2))
    if summary['pages_ok'] != len(results):
        raise SystemExit('OCR measurement includes failed pages')


if __name__ == '__main__':
    main()
