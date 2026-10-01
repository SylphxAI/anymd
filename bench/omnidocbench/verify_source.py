#!/usr/bin/env python3
"""Verify reused prediction provenance with three one-shot CI API reads, no polling."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


def api(path):
    return subprocess.check_output(['gh', 'api', '--allow-escape-sequences', path], text=True)


def validate(run, jobs, log, sha, engine, limit):
    if run['head_sha'] != sha or run['path'] != '.github/workflows/omnidocbench.yml':
        raise ValueError('Prediction source SHA or workflow does not match')
    predictions = [j for j in jobs if re.fullmatch(r'Pages \d+/32', j['name'])]
    expected = {f'Pages {i}/32' for i in range(1, 33)}
    if len(predictions) != 32 or {j['name'] for j in predictions} != expected:
        raise ValueError('Prediction source does not contain all 32 shards')
    if any(j['status'] != 'completed' or j['conclusion'] != 'success' for j in predictions):
        raise ValueError('Every prediction shard must have completed successfully')
    values = {'ANYMD_OCR': set(), 'LIMIT': set()}
    for line in log.splitlines():
        # GitHub logs prefix each line with an ISO timestamp.
        line = re.sub(r'^\d{4}-\d{2}-\d{2}T\S+Z\s+', '', line).strip()
        for key in values:
            if line.startswith(key + ':'):
                values[key].add(line.split(':', 1)[1].strip())
    if values['ANYMD_OCR'] != {engine} or values['LIMIT'] != {str(limit)}:
        raise ValueError('Prediction source engine or limit is missing or mismatched')
    return {'source_run': run['id'], 'source_sha': sha, 'ocr': engine, 'limit': limit,
            'prediction_shards': 32, 'sample_log_sha256': hashlib.sha256(log.encode()).hexdigest()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--repo', required=True)
    parser.add_argument('--run', required=True, type=int)
    parser.add_argument('--sha', required=True)
    parser.add_argument('--ocr', required=True, choices=['vlm', 'tesseract'])
    parser.add_argument('--limit', required=True, type=int)
    parser.add_argument('--out', required=True)
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{40}', args.sha) or args.limit < 0:
        parser.error('sha must be a full lowercase commit SHA and limit must be nonnegative')
    base = f'repos/{args.repo}/actions'
    run = json.loads(api(f'{base}/runs/{args.run}'))
    jobs = json.loads(api(f'{base}/runs/{args.run}/jobs?per_page=100'))['jobs']
    predictions = [j for j in jobs if re.fullmatch(r'Pages \d+/32', j['name'])]
    complete = [j for j in predictions if j['status'] == 'completed' and j['conclusion'] == 'success']
    if not complete:
        raise SystemExit('No successful source prediction job is available')
    log = api(f"{base}/jobs/{complete[0]['id']}/logs")
    result = validate(run, jobs, log, args.sha, args.ocr, args.limit)
    Path(args.out).write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
