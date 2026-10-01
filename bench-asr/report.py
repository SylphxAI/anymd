#!/usr/bin/env python3
"""Score ASR benchmark results and print the comparison as Markdown.

  report.py RESULTS_DIR [--normalizer-dir DIR] > report.md

RESULTS_DIR holds one sub-directory per job, each with summary.json and results.jsonl
(run_engine.py output). Metrics, as fixed in the decision:
  en  (fleurs-en, librispeech-clean): WER after the Open ASR Leaderboard EnglishTextNormalizer.
  zh / yue / ja: CER after NFKC, dropping punctuation, symbols and spaces, and OpenCC t2s
  on both sides (Cantonese references are Traditional).
Confidence intervals are 95% percentile bootstrap over utterances (1000 resamples).

--cjk-diagnostics additionally shows sensitivity to Latin parenthetical
annotations and numeral spelling (requires cn2an==0.5.24 and kanjize==1.6.1). This is NOT
Qwen's verified scoring protocol: the public report does not specify normalization.
"""

import argparse
import json
import random
import re
import sys
import unicodedata
from collections import defaultdict
from pathlib import Path

PUBLISHED = {  # Qwen3-ASR-1.7B, arXiv 2601.21337, error rate in percent
    "fleurs-en": 3.35,
    "fleurs-zh": 2.41,
    "fleurs-yue": 3.98,
    "fleurs-ja": 5.20,
    "librispeech-clean": 1.63,
}
DATASETS = ["fleurs-en", "fleurs-zh", "fleurs-yue", "fleurs-ja", "librispeech-clean"]


def load_normalizers(directory: Path):
    sys.path.insert(0, str(directory.parent))
    from importlib import import_module

    english = import_module(f"{directory.name}.normalizer").EnglishTextNormalizer()
    from opencc import OpenCC

    t2s = OpenCC("t2s")

    def cjk(text: str) -> str:
        text = unicodedata.normalize("NFKC", text).lower()
        text = "".join(c for c in text if unicodedata.category(c)[0] not in "PSZC")
        return t2s.convert(text)

    return english, cjk


def cjk_diagnostics(cjk, dataset="fleurs-zh"):
    """Return symmetric ablations, not an inferred official normalizer.

    Remove only Latin parenthetical annotations, preserving Chinese glosses and
    numeric parentheses. Numeral conversion is deliberately diagnostic: a generic
    ITN converter can also change lexical numerals (e.g. 一 in a Chinese word).
    """
    def remove_annotation(match):
        content = match.group(0)[1:-1]
        latin = any("LATIN" in unicodedata.name(c, "") for c in content)
        cjk_char = any(
            any(script in unicodedata.name(c, "") for script in ("CJK", "HIRAGANA", "KATAKANA", "HANGUL"))
            for c in content
        )
        return "" if latin and not cjk_char else match.group(0)

    def annotations(text):
        text = unicodedata.normalize("NFKC", text)
        return cjk(re.sub(r"\([^()]*\)", remove_annotation, text))

    def numerals(text):
        if dataset == "fleurs-ja":
            from kanjize import kanji2number

            def convert(match):
                try:
                    return str(kanji2number(match.group(0)))
                except ValueError:
                    return match.group(0)

            return re.sub(r"[〇零一二三四五六七八九十百千万億兆]+", convert, annotations(text))
        import cn2an

        return cn2an.transform(annotations(text), "cn2an")

    return annotations, numerals


def mandarin_diagnostics(cjk):
    return cjk_diagnostics(cjk)


def japanese_kana_diagnostic(cjk):
    """Numeral and katakana/hiragana sensitivity, retaining all annotations.

    This preserves the baseline OpenCC step for comparability. It does not map
    kanji to readings, so lexical errors remain errors.
    """
    from kanjize import kanji2number

    def normalize(text):
        def convert(match):
            try:
                return str(kanji2number(match.group(0)))
            except ValueError:
                return match.group(0)

        text = re.sub(r"[〇零一二三四五六七八九十百千万億兆]+", convert, cjk(text))
        return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in text)

    return normalize


def edit_stats(ref: str, hyp: str, unit: str) -> tuple[int, int]:
    import jiwer

    if not ref:
        return 0, 0
    if not hyp:
        n = len(ref.split()) if unit == "word" else len(ref)
        return n, n
    out = jiwer.process_words(ref, hyp) if unit == "word" else jiwer.process_characters(ref, hyp)
    errors = out.substitutions + out.deletions + out.insertions
    return errors, len(ref.split()) if unit == "word" else len(ref)


def score(rows, dataset, english, cjk):
    unit = "word" if dataset in ("fleurs-en", "librispeech-clean") else "char"
    pairs = []
    for row in rows:
        if unit == "word":
            ref, hyp = english(row["ref"]), english(row["hyp"])
        else:
            ref, hyp = cjk(row["ref"]), cjk(row["hyp"])
        pairs.append(edit_stats(ref, hyp, unit))
    total_e = sum(e for e, _ in pairs)
    total_n = sum(n for _, n in pairs)
    rate = 100 * total_e / max(total_n, 1)
    rng = random.Random(0)
    samples = []
    for _ in range(1000):
        pick = [pairs[rng.randrange(len(pairs))] for _ in pairs]
        n = sum(n for _, n in pick)
        samples.append(100 * sum(e for e, _ in pick) / max(n, 1))
    samples.sort()
    return rate, samples[25], samples[974], unit


def words_sane(rows) -> tuple[int, int]:
    have = ok = 0
    for row in rows:
        words = row.get("words") or []
        if not words:
            continue
        have += 1
        starts = [w["start"] for w in words]
        if starts == sorted(starts) and words[-1]["end"] <= row["dur"] + 1.0 and all(w["end"] >= w["start"] for w in words):
            ok += 1
    return have, ok


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("results")
    parser.add_argument("--probes")
    parser.add_argument("--cjk-diagnostics", "--mandarin-diagnostics", dest="cjk_diagnostics", action="store_true")
    parser.add_argument("--normalizer-dir", default=str(Path(__file__).parent / "oanorm"))
    args = parser.parse_args()
    english, cjk = load_normalizers(Path(args.normalizer_dir).resolve())

    jobs = []
    for summary_path in sorted(Path(args.results).rglob("summary.json")):
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        results = summary_path.with_name("results.jsonl")
        rows = [json.loads(l) for l in results.read_text(encoding="utf-8").splitlines() if l.strip()] if results.exists() else []
        jobs.append((summary, rows))

    print("# ASR benchmark results\n")
    failed = [s for s, _ in jobs if s.get("status") != "ok"]
    if failed:
        print("## Failed jobs\n")
        for s in failed:
            print(f"- `{s['label']}` on {s['os']} {s['dataset']}: `{(s.get('error') or '').splitlines()[0] if s.get('error') else 'failed'}`")
        print()

    ok = [(s, r) for s, r in jobs if s.get("status") == "ok" and r]
    accuracy = defaultdict(dict)  # (label, os, aligner) -> dataset -> (rate, lo, hi, n)
    for s, rows in ok:
        key = (s["label"], s["os"])
        accuracy[key][s["dataset"]] = score(rows, s["dataset"], english, cjk)[:3] + (s["n"],)

    print("## Accuracy (error rate %, lower is better; 95% CI in brackets)\n")
    print("WER for English, CER for zh, yue and ja. Same samples for every engine (fixed seed, pinned revisions).\n")
    print("| Engine | OS | " + " | ".join(DATASETS) + " |")
    print("|---|---|" + "---|" * len(DATASETS))
    for (label, os_), per in sorted(accuracy.items()):
        cells = []
        for d in DATASETS:
            if d in per:
                rate, lo, hi, n = per[d]
                cells.append(f"{rate:.2f} [{lo:.2f}, {hi:.2f}] (n={n})")
            else:
                cells.append("-")
        print(f"| {label} | {os_} | " + " | ".join(cells) + " |")
    print("| _Qwen3-ASR-1.7B published_ | | " + " | ".join(f"{PUBLISHED[d]:.2f}" for d in DATASETS) + " |\n")

    print("## Comparison with Qwen's published numbers (Linux, sampled utterances)\n")
    print("Reference only, not a verified reproduction gate. [Qwen's evaluation settings](https://github.com/QwenLM/Qwen3-ASR/tree/7c6daf77a2421100f5fb066495372c00129d39ff#evaluation) are bfloat16, vLLM, greedy decoding, max_new_tokens=1024 and no language parameter. These runs use CPU quantized engines; see each summary's lang_mode. The [technical report](https://arxiv.org/html/2601.21337v2) specifies CER for Mandarin/Cantonese but does not publish the text normalization protocol. Our 200-utterance samples are not the complete test sets. Within 0.5 is only a numerical comparison; its CI covers sampling, not protocol mismatch.\n")

    if args.cjk_diagnostics:
        print("### CJK normalization sensitivity (not verified Qwen protocol)\n")
        print("Both reference and hypothesis receive the same transformations. Remove Latin parenthetical annotations before punctuation stripping; then optionally convert Chinese numeral spellings with cn2an 0.5.24 (zh/yue) or kanjize 1.6.1 (ja). These ablations can remove spoken annotations or alter lexical numerals; they do not prove the audio's ground truth or official score parity. English remains unchanged. Japanese kana/kanji readings are not conflated in this table.\n")
        print("| Engine | OS | Dataset | n | Raw CER (baseline) | Without Latin parentheses | Plus numeral ITN (95% CI) |")
        print("|---|---|---|---|---|---|---|")
        for s, rows in sorted(ok, key=lambda j: (j[0]["label"], j[0]["os"], j[0]["dataset"])):
            if s["dataset"] not in ("fleurs-zh", "fleurs-yue", "fleurs-ja"):
                continue
            dataset = s["dataset"]
            annotations, numerals = cjk_diagnostics(cjk, dataset)
            original = score(rows, dataset, english, cjk)[0]
            without = score(rows, dataset, english, annotations)[0]
            rate, lo, hi, _ = score(rows, dataset, english, numerals)
            print(f"| {s['label']} | {s['os']} | {dataset} | {len(rows)} | {original:.2f} | {without:.2f} | {rate:.2f} [{lo:.2f}, {hi:.2f}] |")
        print()
        print("### Japanese numeral and kana sensitivity (annotations retained)\n")
        print("Baseline NFKC/punctuation/space/OpenCC scoring, then kanjize numeral conversion and katakana-to-hiragana folding on both sides. Kanji readings are not collapsed. This is not official Qwen scoring.\n")
        print("| Engine | OS | n | Numerals + kana CER |")
        print("|---|---|---|---|")
        kana = japanese_kana_diagnostic(cjk)
        for s, rows in sorted(ok, key=lambda j: (j[0]["label"], j[0]["os"])):
            if s["dataset"] == "fleurs-ja":
                rate = score(rows, "fleurs-ja", english, kana)[0]
                print(f"| {s['label']} | {s['os']} | {len(rows)} | {rate:.2f} |")
        print()

    print("| Engine | Dataset | Measured | Published | Diff | Published inside CI | Within 0.5 |")
    print("|---|---|---|---|---|---|---|")
    for (label, os_), per in sorted(accuracy.items()):
        if os_ != "Linux" or "whisper" in label or "Aligner" in label:
            continue
        for d in DATASETS:
            if d in per:
                rate, lo, hi, n = per[d]
                diff = rate - PUBLISHED[d]
                print(f"| {label} | {d} | {rate:.2f} | {PUBLISHED[d]:.2f} | {diff:+.2f} | {'yes' if lo <= PUBLISHED[d] <= hi else 'no'} | {'yes' if abs(diff) <= 0.5 else 'no'} |")
    print()

    print("## Speed and memory\n")
    print("RTF = compute time / audio time (lower is faster), CPU only, 4 threads. `excl. load` removes the model load (one-clip run subtracted); `incl. load` keeps it. Peak RSS is the largest resident set of the runs.\n")
    print("| Engine | OS | Dataset | Utts | Audio (min) | RTF excl. load | RTF incl. load | Peak RSS (MB) | Runner CPU |")
    print("|---|---|---|---|---|---|---|---|---|")
    for s, _ in sorted((j for j in jobs if j[0].get("status") == "ok"), key=lambda j: (j[0]["label"], j[0]["os"], j[0]["dataset"])):
        excl = f"{s['rtf_excl']:.3f}" if s.get("rtf_excl") else "-"
        print(f"| {s['label']} | {s['os']} | {s['dataset']} | {s['n']} | {s['audio_s'] / 60:.1f} | {excl} | {s['rtf_incl']:.3f} | {s['rss_mb']:.0f} | {s['cpu']} ({s['cpus']} vCPU) |")
    print()

    print("## Binary footprint\n")
    print("CLI executable plus the shared libraries shipped beside it, unstripped, as released or built by the workflow. This bounds the size a linked engine adds to anymd; model weights are never embedded.\n")
    print("| Engine | Executable (MB) | Shared libs (MB) | Total (MB) |")
    print("|---|---|---|---|")
    seen = set()
    for s, _ in sorted(jobs, key=lambda j: (j[0]["label"], j[0]["os"])):
        key = (s["label"], s["os"])
        if key in seen or s.get("status") != "ok":
            continue
        seen.add(key)
        print(f"| {s['label']} ({s['os']}) | {s['exe_bytes'] / 1e6:.1f} | {s['libs_bytes'] / 1e6:.1f} | {(s['exe_bytes'] + s['libs_bytes']) / 1e6:.1f} |")
    print()

    if args.probes:
        probes = defaultdict(dict)
        for path in sorted(Path(args.probes).rglob("probe.json")):
            item = json.loads(path.read_text())
            probes[item["crate"]][item["target"]] = item["bytes"]
        targets = sorted({t for per in probes.values() for t in per})
        print("## Rust crates: build on every release target, and bytes added\n")
        print("A minimal binary that links and loads the engine (release, thin LTO, stripped). `FAIL` means the crate did not build on that target (see the run log). Baseline is an empty binary; the difference is what the engine adds to anymd.\n")
        print("| Crate | " + " | ".join(targets) + " |")
        print("|---|" + "---|" * len(targets))
        for crate in ["baseline", "transcribe-cpp", "crispasr", "sherpa-onnx"]:
            if crate not in probes:
                continue
            row = []
            for t in targets:
                b = probes.get(crate, {}).get(t)
                base = probes.get("baseline", {}).get(t)
                if b is None:
                    row.append("-" if crate == "baseline" else "FAIL")
                elif crate == "baseline" or base is None:
                    row.append(f"{b / 1e6:.1f} MB")
                else:
                    row.append(f"{b / 1e6:.1f} MB (+{(b - base) / 1e6:.1f})")
            print(f"| {crate} | " + " | ".join(row) + " |")
        print()

    print("## Timestamps\n")
    print("Share of utterances with each timestamp granularity in the engine's own output; for word timestamps, how many are monotonic and inside the audio.\n")
    print("| Engine | OS | Utts | word | segment | none | word timestamps sane |")
    print("|---|---|---|---|---|---|---|")
    stamps = defaultdict(lambda: [0, 0, 0, 0, 0, 0])
    for s, rows in ok:
        acc = stamps[(s["label"], s["os"])]
        for row in rows:
            acc[0] += 1
            acc[{"word": 1, "segment": 2}.get(row["ts"], 3)] += 1
        have, good = words_sane(rows)
        acc[4] += have
        acc[5] += good
    for (label, os_), acc in sorted(stamps.items()):
        print(f"| {label} | {os_} | {acc[0]} | {acc[1]} | {acc[2]} | {acc[3]} | {acc[5]}/{acc[4]} |")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
