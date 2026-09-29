#!/usr/bin/env python3
"""Build the job matrices for asr-benchmark.yml from the workflow inputs (environment)."""

import json
import os

ENGINES = {
    "whisper": {
        "label": "whisper.cpp large-v3-turbo (control)",
        "engine": "whisper",
        "tool": "build:whisper",
        "tool_key": "whisper",
        "model": "whisper-large-v3-turbo",
        "model_file": "ggml-large-v3-turbo.bin",
    },
    "crispasr": {
        "label": "crispasr Qwen3-ASR-1.7B Q8_0",
        "engine": "crispasr",
        "tool": "crispasr",
        "tool_key": "crispasr",
        "model": "crispasr-qwen3-1.7b-q8",
        "model_file": "qwen3-asr-1.7b-q8_0.gguf",
    },
    "transcribe-cpp": {
        "label": "transcribe-cpp Qwen3-ASR-1.7B Q8_0",
        "engine": "transcribe-cpp",
        "tool": "build:transcribe-cpp",
        "tool_key": "transcribe-cpp",
        "model": "transcribe-qwen3-1.7b-q8",
        "model_file": "Qwen3-ASR-1.7B-Q8_0.gguf",
    },
    "sherpa-onnx": {
        "label": "sherpa-onnx Qwen3-ASR-1.7B int8 (community export)",
        "engine": "sherpa-onnx",
        "tool": "sherpa",
        "tool_key": "sherpa",
        "model": "sherpa-qwen3-1.7b-int8",
        "model_file": "",
    },
}
ALIGNED = {
    **ENGINES["crispasr"],
    "label": "crispasr Qwen3-ASR-1.7B Q8_0 + Qwen3-ForcedAligner-0.6B Q8_0",
    "aligner": "qwen3-aligner-0.6b-q8",
    "aligner_file": "qwen3-forced-aligner-0.6b-q8_0.gguf",
}
LINUX_DATASETS = ["fleurs-en", "fleurs-zh", "fleurs-yue", "fleurs-ja", "librispeech-clean"]
MACOS_DATASETS = ["fleurs-en", "fleurs-yue"]
ALIGN_DATASETS = ["fleurs-en", "fleurs-zh", "fleurs-yue", "fleurs-ja"]
RUNNER = {"linux": "ubuntu-latest", "macos": "macos-latest"}


def main() -> None:
    smoke = os.environ.get("EVENT") == "pull_request"
    wanted = [e.strip() for e in os.environ.get("ENGINES", "").split(",") if e.strip()] or list(ENGINES)
    unknown = [e for e in wanted if e not in ENGINES]
    if unknown:
        raise SystemExit(f"unknown engines: {unknown}")
    utts = 3 if smoke else int(os.environ.get("UTTS") or 200)
    macos_utts = 3 if smoke else int(os.environ.get("MACOS_UTTS") or 40)
    lang_mode = os.environ.get("LANG_MODE") or "explicit"

    matrix, builds, datasets = [], [], set()

    def add(spec, osk, dataset, limit):
        datasets.add(dataset)
        slug = spec["engine"] + ("-aligner" if spec.get("aligner") else "")
        matrix.append(
            {
                "aligner": "",
                "aligner_file": "",
                **spec,
                "id": f"{slug}-{osk}-{dataset}",
                "osk": osk,
                "runner": RUNNER[osk],
                "dataset": dataset,
                "limit": limit,
                "lang_mode": lang_mode,
            }
        )

    for name in wanted:
        spec = ENGINES[name]
        for dataset in LINUX_DATASETS:
            add(spec, "linux", dataset, 0)
        if macos_utts:
            for dataset in MACOS_DATASETS:
                add(spec, "macos", dataset, macos_utts)
    if "crispasr" in wanted:
        for dataset in ALIGN_DATASETS:
            add(ALIGNED, "linux", dataset, min(utts, 20))
    for name in wanted:
        tool = ENGINES[name]["tool"]
        if tool.startswith("build:"):
            for osk in ("linux", "macos") if macos_utts else ("linux",):
                builds.append({"tool": tool.split(":", 1)[1], "osk": osk, "runner": RUNNER[osk]})
    out = {
        "matrix": json.dumps(matrix),
        "builds": json.dumps(builds),
        "datasets": json.dumps(sorted(datasets)),
        "utts": str(max(utts, macos_utts) if not smoke else 3),
    }
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as fh:
        for key, value in out.items():
            fh.write(f"{key}={value}\n")
    print(json.dumps({k: json.loads(v) if v.startswith(("[", "{")) else v for k, v in out.items()}, indent=1)[:4000])


if __name__ == "__main__":
    main()
