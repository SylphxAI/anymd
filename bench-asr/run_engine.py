#!/usr/bin/env python3
"""Run one speech-to-text engine over a dataset manifest and record speed and memory.

  run_engine.py ENGINE --manifest M --out DIR --tool-dir T --model PATH [options]

ENGINE: whisper (whisper.cpp CLI, the control), crispasr, transcribe-cpp, sherpa-onnx.

Speed is measured as: one process on the first clip (model load + one clip), then one
process on the whole batch. rtf_excl = (T_all - T_one) / (A_all - A_one) removes the model
load; rtf_incl = T_all / A_all keeps it. Peak RSS is the largest resident set of either
process. Outputs: DIR/results.jsonl (one row per utterance) and DIR/summary.json.
"""

import argparse
import json
import os
import platform
import resource
import subprocess
import sys
import time
from pathlib import Path

EXE = {
    "whisper": "whisper-cli",
    "crispasr": "crispasr",
    "transcribe-cpp": "transcribe-cli",
    "sherpa-onnx": "sherpa-onnx-offline",
}


def find_exe(tool_dir: Path, name: str) -> Path:
    for path in sorted(tool_dir.rglob(name)):
        if path.is_file() and os.access(path, os.X_OK):
            return path
    raise SystemExit(f"{name} not found under {tool_dir}")


def lib_env(tool_dir: Path) -> dict:
    dirs = sorted({str(p.parent) for p in tool_dir.rglob("*") if p.is_file() and (".so" in p.name or p.name.endswith(".dylib"))})
    env = dict(os.environ)
    if dirs:
        joined = os.pathsep.join(dirs)
        for key in ("LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"):
            env[key] = joined + (os.pathsep + env[key] if env.get(key) else "")
    return env


def footprint(tool_dir: Path, exe: Path) -> dict:
    libs = [p for p in tool_dir.rglob("*") if p.is_file() and (".so" in p.name or p.name.endswith(".dylib"))]
    return {"exe_bytes": exe.stat().st_size, "libs_bytes": sum(p.stat().st_size for p in libs), "libs": len(libs)}


def cpu_model() -> str:
    try:
        if sys.platform == "darwin":
            return subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor()


def rss_mb() -> float:
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    return peak / (1024 * 1024) if sys.platform == "darwin" else peak / 1024


def text_of(segments) -> str:
    return "".join(s.get("text", "") for s in segments).strip()


def whisper_json(path: Path) -> tuple[str, str, list]:
    data = json.loads(path.read_text(encoding="utf-8"))
    segments = data["transcription"]
    if not isinstance(segments, list) or any(not isinstance(s.get("text"), str) for s in segments):
        raise ValueError("invalid transcript output; an empty transcription list is allowed")
    kind = "segment" if any("offsets" in s for s in segments) else "none"
    words = []
    for s in segments:
        for w in s.get("words") or []:
            words.append({"text": w.get("text", ""), "start": w["offsets"]["from"] / 1000, "end": w["offsets"]["to"] / 1000})
    if words:
        kind = "word"
    return text_of(segments), kind, words


def run(engine: str, args, wavs: list[Path], lang: str, work: Path):
    """One process over `wavs`. Returns (results by wav path, load_ms or None, seconds, log tail)."""
    tool = Path(args.tool_dir)
    exe = find_exe(tool, EXE[engine])
    env = lib_env(tool)
    threads = str(args.threads)
    lang_arg = "auto" if args.lang_mode == "auto" else lang
    for wav in wavs:  # stale outputs from an earlier process must not be mistaken for this one's
        for stale in (wav.with_suffix(".json"), wav.with_name(wav.name + ".json")):
            stale.unlink(missing_ok=True)
    load_ms = None
    if engine == "whisper":
        cmd = [str(exe), "-m", args.model, "-l", lang_arg, "-t", threads, "-oj", "-ng", "-np", *map(str, wavs)]
    elif engine == "crispasr":
        cmd = [str(exe), "--backend", "qwen3", "-m", args.model, "-l", lang_arg, "-t", threads, "-oj", "-np", "-ng"]
        if args.aligner:
            cmd += ["-am", args.aligner, "-ojf"]
        for wav in wavs:
            cmd += ["-f", str(wav)]
    elif engine == "transcribe-cpp":
        listing = work / "batch.list"
        listing.write_text("".join(f"{w}\n" for w in wavs))
        cmd = [str(exe), "-m", args.model, "--batch", str(listing), "--batch-jsonl", "--threads", threads, "--backend", "cpu"]
        if lang_arg != "auto":
            cmd += ["-l", lang_arg]
    elif engine == "sherpa-onnx":
        model = Path(args.model)
        cmd = [
            str(exe),
            f"--qwen3-asr-conv-frontend={model / 'conv_frontend.onnx'}",
            f"--qwen3-asr-encoder={model / 'encoder.int8.onnx'}",
            f"--qwen3-asr-decoder={model / 'decoder.int8.onnx'}",
            f"--qwen3-asr-tokenizer={model / 'tokenizer'}",
            f"--num-threads={threads}",
            "--print-args=false",
            *map(str, wavs),
        ]
    else:
        raise SystemExit(f"unknown engine {engine}")
    started = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env, timeout=args.timeout)
    seconds = time.perf_counter() - started
    tail = "\n".join((proc.stderr + "\n" + proc.stdout).strip().splitlines()[-25:])
    if proc.returncode != 0:
        raise RuntimeError(f"{engine} exited {proc.returncode}\n{tail}")
    out: dict[Path, tuple[str, str, list]] = {}
    if engine == "whisper":
        for wav in wavs:
            path = wav.with_name(wav.name + ".json")
            if path.exists():
                out[wav] = whisper_json(path)
    elif engine == "crispasr":
        for wav in wavs:
            path = wav.with_suffix(".json")
            if path.exists():
                out[wav] = whisper_json(path)
    elif engine == "transcribe-cpp":
        for line in proc.stdout.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            row = json.loads(line)
            if row.get("type") == "batch_header":
                load_ms = row.get("load_ms")
                continue
            segs = row.get("segments") or []
            path = Path(row["file"])
            if path in out or not isinstance(row.get("text"), str):
                raise ValueError("duplicate or malformed transcript output")
            out[path] = (row["text"].strip(), "segment" if segs else "none", [])
    else:
        # sherpa-onnx prints every input path first, then one JSON object per input, in order.
        lines = (proc.stdout + "\n" + proc.stderr).splitlines()  # sherpa-onnx logs to stderr
        requested = {str(w) for w in wavs}
        paths = [Path(l.strip()) for l in lines if l.strip() in requested]
        rows = []
        for line in lines:
            line = line.strip()
            if line.startswith("{"):
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
        if len(paths) != len(rows) or len(paths) != len(set(paths)):
            raise ValueError("missing or duplicate sherpa transcript output")
        for path, row in zip(paths, rows):
            if not isinstance(row.get("text"), str):
                raise ValueError("malformed sherpa transcript output")
            out[path] = (row["text"].strip(), "word" if row.get("timestamps") else "none", [])
    return out, load_ms, seconds, tail


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("engine", choices=sorted(EXE))
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--tool-dir", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--aligner")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--lang-mode", choices=["explicit", "auto"], default="explicit")
    parser.add_argument("--label", default="")
    parser.add_argument("--job-id", default="", help="identity from the selected workflow matrix")
    parser.add_argument("--timeout", type=int, default=5 * 3600)
    args = parser.parse_args()

    items = [json.loads(l) for l in Path(args.manifest).read_text(encoding="utf-8").splitlines() if l.strip()]
    if args.limit:
        items = items[: args.limit]
    lang = items[0]["lang"]
    wavs = [(Path(args.manifest).parent / i["wav"]).resolve() for i in items]
    ids = [i["id"] for i in items]
    if len(ids) != len(set(ids)) or len(wavs) != len(set(wavs)):
        raise ValueError("duplicate utterance IDs or audio paths")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    work = out / "work"
    work.mkdir(exist_ok=True)
    tool = Path(args.tool_dir)
    exe = find_exe(tool, EXE[args.engine])
    summary = {
        "engine": args.engine,
        "job_id": args.job_id,
        "label": args.label or args.engine,
        "os": platform.system(),
        "arch": platform.machine(),
        "cpu": cpu_model(),
        "cpus": os.cpu_count(),
        "threads": args.threads,
        "dataset": items[0]["dataset"],
        "lang": lang,
        "lang_mode": args.lang_mode,
        "model": Path(args.model).name,
        "aligner": bool(args.aligner),
        "n": len(items),
        "expected_ids": ids,
        "audio_s": sum(i["dur"] for i in items),
        **footprint(tool, exe),
        "status": "ok",
    }
    try:
        # Calibration: the shortest clip, so the load time dominates.
        first = min(range(len(items)), key=lambda k: items[k]["dur"])
        calibration, load_one, t_one, _ = run(args.engine, args, [wavs[first]], lang, work)
        if set(calibration) != {wavs[first]}:
            raise ValueError("incomplete calibration transcript output")
        rss_one = rss_mb()
        results, load_all, t_all, tail = run(args.engine, args, wavs, lang, work)
        rss_all = rss_mb()
        a_one, a_all = items[first]["dur"], summary["audio_s"]
        complete = set(results) == set(wavs)
        summary.update(
            t_one=t_one,
            a_one=a_one,
            t_all=t_all,
            rtf_incl=t_all / a_all if complete else None,
            rtf_excl=(t_all - t_one) / (a_all - a_one) if complete and a_all > a_one and t_all > t_one else None,
            load_s=(load_all / 1000) if load_all else None,
            rss_mb=rss_all,
            rss_mb_one=rss_one,
        )
    except Exception as exc:  # noqa: BLE001 - recorded, then the step fails
        summary.update(status="failed", error=str(exc)[-3000:])
        (out / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False))
        print(summary["error"], file=sys.stderr)
        return 1
    missing = 0
    with (out / "results.jsonl").open("w", encoding="utf-8") as fh:
        for item, wav in zip(items, wavs):
            hit = results.get(wav)
            if hit is None:
                missing += 1
                hit = ("", "none", [])
            hyp, kind, words = hit
            fh.write(json.dumps({"id": item["id"], "status": "missing" if wav not in results else "ok",
                                 "lang": item["lang"], "dur": item["dur"], "ref": item["ref"], "hyp": hyp,
                                 "ts": kind, "words": words}, ensure_ascii=False) + "\n")
    summary["missing"] = missing
    if not complete:
        summary.update(status="failed", error="missing or unexpected batch transcript output",
                       unexpected=len(set(results) - set(wavs)))
    (out / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False))
    print(json.dumps(summary, indent=2, ensure_ascii=False))
    print(tail, file=sys.stderr)
    return 0 if complete else 1


if __name__ == "__main__":
    raise SystemExit(main())
