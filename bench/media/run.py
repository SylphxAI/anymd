#!/usr/bin/env python3
"""Measure embedded subtitle extraction; no ASR or visual captioning is involved."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import time


def execute(command):
    started = time.perf_counter()
    process = subprocess.run(command, capture_output=True, text=True, timeout=120)
    return process.stdout, process.stderr, process.returncode, time.perf_counter() - started


def normalize(text):
    return " ".join(text.split())


def srt_cues(text):
    cues = []
    for block in text.strip().split("\n\n"):
        lines = block.splitlines()
        for index, line in enumerate(lines):
            match = re.match(r"(\d+):(\d+):(\d+)[,.]\d+\s+-->", line)
            if match:
                h, m, s = map(int, match.groups())
                cues.append((h * 3600 + m * 60 + s, normalize(" ".join(lines[index + 1:]))))
                break
    return cues


def markdown_cues(text):
    cues = []
    for line in text.splitlines():
        match = re.fullmatch(r"\[(\d+:\d+(?::\d+)?)\] (.+)", line)
        if match:
            parts = list(map(int, match[1].split(":")))
            seconds = 0
            for part in parts:
                seconds = seconds * 60 + part
            cues.append((seconds, normalize(match[2])))
    return cues


def score(expected, actual):
    """Exact text + start-second matches; multiset counting penalizes duplicates."""
    matches = sum((Counter(expected) & Counter(actual)).values())
    return {"matched": matches, "expected": len(expected), "extracted": len(actual)}


def make_srt(cues):
    return "\n\n".join(
        f"{index + 1}\n00:00:{index:02d},000 --> 00:00:{index:02d},900\n{text}"
        for index, text in enumerate(cues)
    ) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--anymd", required=True)
    parser.add_argument("--work", type=Path, required=True, help="New, empty generated-media directory")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=False)
    corpus_path = Path(__file__).with_name("corpus.json")
    corpus = json.loads(corpus_path.read_text())
    versions = {}
    for tool, command in [("anymd", [args.anymd, "version"]), ("ffmpeg", ["ffmpeg", "-version"])]:
        stdout, stderr, rc, _ = execute(command)
        if rc:
            raise RuntimeError(f"{tool} version failed: {stderr}")
        versions[tool] = stdout.splitlines()[0]
    rows = []
    for dataset in corpus["datasets"]:
        for case in corpus["cases"]:
            stem = f"{dataset['id']}-{case['id']}"
            # The source subtitle has a different stem so anymd cannot read it as a sidecar.
            subtitle = args.work / f"source-{stem}.srt"
            subtitle.write_text(make_srt(case["cues"]))
            video = args.work / f"{stem}.{dataset['extension']}"
            command = ["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i",
                       "color=c=black:s=160x90:r=10:d=3", "-i", str(subtitle),
                       "-map", "0:v:0", "-map", "1:s:0", "-c:v", "mpeg4",
                       "-c:s", dataset["subtitle_codec"], "-t", "3", str(video)]
            _, stderr, rc, _ = execute(command)
            if rc:
                raise RuntimeError(f"fixture generation failed: {stderr}")
            expected = [(index, normalize(text)) for index, text in enumerate(case["cues"])]
            for tool in ("anymd", "ffmpeg"):
                command = [args.anymd, str(video)] if tool == "anymd" else [
                    "ffmpeg", "-nostdin", "-v", "error", "-i", str(video),
                    "-map", "0:s:0", "-c:s", "srt", "-f", "srt", "-"]
                stdout, stderr, rc, seconds = execute(command)
                actual = markdown_cues(stdout) if tool == "anymd" else srt_cues(stdout)
                output = args.work / f"{stem}-{tool}.txt"
                output.write_text(stdout)
                rows.append({"dataset": dataset["id"], "case": case["id"], "tool": tool,
                             "exit_code": rc, "stderr": stderr, "seconds": seconds,
                             "video_sha256": hashlib.sha256(video.read_bytes()).hexdigest(),
                             "output": stdout, **score(expected, actual)})
    result = {"measured_at": datetime.now(timezone.utc).isoformat(), "versions": versions,
              "platform": platform.platform(), "cpu": platform.processor(),
              "corpus_sha256": hashlib.sha256(corpus_path.read_bytes()).hexdigest(),
              "timing": "One process per clip, sequential; includes process startup. No warmup or repeat. Fixture muxing excluded.",
              "rows": rows}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"versions": versions, "rows": len(rows)}, indent=2))


if __name__ == "__main__":
    main()
