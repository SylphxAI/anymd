#!/usr/bin/env python3
"""Prepare one ASR benchmark dataset: pinned revision, fixed-seed sample, 16 kHz mono wav.

  fetch_data.py DATASET OUT_DIR [--utts 200]

DATASET is one of fleurs-en, fleurs-zh, fleurs-yue, fleurs-ja, librispeech-clean.
Writes OUT_DIR/manifest.jsonl (id, dataset, lang, wav relative to OUT_DIR, ref, dur) and OUT_DIR/wav/*.wav.
Every source is pinned to a Hugging Face dataset commit, so a rerun reads the same audio.
"""

import argparse
import csv
import io
import json
import random
import subprocess
import sys
import tarfile
import urllib.request
import wave
from pathlib import Path

SEED = 20260929
FLEURS_REV = "70bb2e84b976b7e960aa89f1c648e09c59f894dd"  # google/fleurs, CC-BY-4.0
LIBRI_REV = "71cacbfb7e2354c4226d01e70d77d5fca3d04ba1"  # openslr/librispeech_asr, CC-BY-4.0

FLEURS = {
    "fleurs-en": ("en_us", "en"),
    "fleurs-zh": ("cmn_hans_cn", "zh"),
    "fleurs-yue": ("yue_hant_hk", "yue"),
    "fleurs-ja": ("ja_jp", "ja"),
}


def download(url: str, dest: Path) -> None:
    subprocess.run(
        ["curl", "-fL", "--retry", "6", "--retry-delay", "10", "--retry-all-errors", "-o", str(dest), url],
        check=True,
    )


def duration(path: Path) -> float:
    with wave.open(str(path), "rb") as w:
        return w.getnframes() / w.getframerate()


def to_wav16(src: bytes, dest: Path) -> None:
    subprocess.run(
        ["ffmpeg", "-nostdin", "-loglevel", "error", "-y", "-i", "pipe:0", "-ac", "1", "-ar", "16000", "-f", "wav", str(dest)],
        input=src,
        check=True,
    )


def fleurs(dataset: str, out: Path, utts: int) -> list[dict]:
    config, lang = FLEURS[dataset]
    base = f"https://huggingface.co/datasets/google/fleurs/resolve/{FLEURS_REV}/data/{config}"
    tsv = out / "test.tsv"
    download(f"{base}/test.tsv", tsv)
    rows = []
    with tsv.open(encoding="utf-8", newline="") as fh:
        for row in csv.reader(fh, delimiter="\t"):
            # id, file_name, raw_transcription, transcription, phonemes, num_samples, gender
            rows.append({"id": row[0], "file": row[1], "ref": row[2]})
    rows.sort(key=lambda r: (r["id"], r["file"]))
    chosen = random.Random(SEED).sample(rows, min(utts, len(rows)))
    chosen.sort(key=lambda r: (r["id"], r["file"]))
    wanted = {r["file"]: r for r in chosen}
    archive = out / "test.tar.gz"
    download(f"{base}/audio/test.tar.gz", archive)
    manifest = []
    (out / "wav").mkdir(exist_ok=True)
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar:
            name = Path(member.name).name
            row = wanted.get(name)
            if row is None or not member.isfile():
                continue
            data = tar.extractfile(member).read()
            uid = f"{dataset}-{Path(row['file']).stem}"
            wav = out / "wav" / f"{uid}.wav"
            to_wav16(data, wav)
            manifest.append(
                {"id": uid, "dataset": dataset, "lang": lang, "wav": f"wav/{wav.name}", "ref": row["ref"], "dur": duration(wav)}
            )
    archive.unlink()
    return manifest


def librispeech(out: Path, utts: int) -> list[dict]:
    import pyarrow.parquet as pq

    url = f"https://huggingface.co/datasets/openslr/librispeech_asr/resolve/{LIBRI_REV}/all/test.clean/0000.parquet"
    parquet = out / "test.clean.parquet"
    download(url, parquet)
    table = pq.read_table(parquet).to_pylist()
    table.sort(key=lambda r: r["id"])
    chosen = random.Random(SEED).sample(table, min(utts, len(table)))
    chosen.sort(key=lambda r: r["id"])
    manifest = []
    (out / "wav").mkdir(exist_ok=True)
    for row in chosen:
        wav = out / "wav" / f"librispeech-{row['id']}.wav"
        to_wav16(row["audio"]["bytes"], wav)
        manifest.append(
            {"id": f"librispeech-{row['id']}", "dataset": "librispeech-clean", "lang": "en", "wav": f"wav/{wav.name}", "ref": row["text"], "dur": duration(wav)}
        )
    parquet.unlink()
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("dataset")
    parser.add_argument("out")
    parser.add_argument("--utts", type=int, default=200)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    if args.dataset in FLEURS:
        manifest = fleurs(args.dataset, out, args.utts)
    elif args.dataset == "librispeech-clean":
        manifest = librispeech(out, args.utts)
    else:
        print(f"unknown dataset {args.dataset}", file=sys.stderr)
        return 2
    manifest.sort(key=lambda m: m["id"])
    if not manifest:
        print("no utterances selected", file=sys.stderr)
        return 1
    with (out / "manifest.jsonl").open("w", encoding="utf-8") as fh:
        for item in manifest:
            fh.write(json.dumps(item, ensure_ascii=False) + "\n")
    total = sum(m["dur"] for m in manifest)
    print(f"{args.dataset}: {len(manifest)} utterances, {total / 60:.1f} min")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
