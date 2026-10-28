"""Model-free real CLI/MCP subtitle regression and an optional historical rerun.

Reruns the original bench/media corpus (four cases in each of two containers),
then NEW sidecar/edge-case regressions. It does not overwrite historical results.
--corpus can supply existing original clips instead of regenerating them.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import html
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import tempfile
import threading


# (start_ms, end_ms, SRT payload, expected cleaned payload, optional VTT payload)
# Eight cases, twelve authored text/start pairs per container.
CASES = {
    "repeated": [(0, 900, "Again", "Again", None),
                 (1000, 1900, "Again", "Again", None)],
    "overlap": [(0, 2000, "shared", "shared", None),
                (1250, 3000, "shared\nnew", "shared new", None)],
    "multiline": [(0, 900, "echo\necho", "echo echo", None),
                  (1000, 1900, "echo\necho\nmore", "echo echo more", None)],
    "cjk": [(0, 900, "你好，世界 日本語 한국어", "你好，世界 日本語 한국어", None),
            (1000, 1900, "你好，世界 日本語 한국어", "你好，世界 日本語 한국어", None)],
    "markup": [(0, 900, r"{\an8}<i>Hi &amp; bye</i>", "Hi & bye", None)],
    "voice": [(0, 900, "Alice: Hello", "Alice: Hello", "<v.speaker Alice><b>Hello</b></v>")],
    "fractional": [(125, 975, "Precise", "Precise", None)],
    "gap": [(3_456, 4_000, "Late", "Late", None)],
}


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, capture_output=True,
                          text=True, encoding="utf-8", timeout=60, **kwargs).stdout


def clock(ms, separator="."):
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02}{separator}{ms % 1000:03}"


def milliseconds(value):
    parts = value.replace(",", ".").split(":")
    seconds, fraction = parts[-1].split(".") if "." in parts[-1] else (parts[-1], "0")
    return (int(parts[-3]) * 3600000 if len(parts) == 3 else 0) + int(parts[-2]) * 60000 + int(seconds) * 1000 + int(fraction.ljust(3, "0"))


def markdown_pairs(text):
    return [[milliseconds(m[1]), m[2]] for m in re.finditer(
        r"^\[(\d{2,}:\d{2}(?::\d{2})?(?:\.\d{3})?)\] (.*)$", text, re.M)]


def ffmpeg_pairs(source):
    """Independent FFmpeg demux/decode, with only presentation markup removed."""
    text = run(["ffmpeg", "-nostdin", "-v", "error", "-copyts", "-i", source,
                "-map", "0:s:0", "-c:s", "srt", "-f", "srt", "-"])
    pairs = []
    for block in re.split(r"\n\s*\n", text.strip()):
        lines = block.splitlines()
        index = next((i for i, line in enumerate(lines) if " --> " in line), None)
        if index is None:
            continue
        payload = re.sub(r"<[^>]*>|\{\\[^}]*\}", "", " ".join(lines[index + 1:]))
        payload = " ".join(html.unescape(payload).split())
        if payload:
            pairs.append([milliseconds(lines[index].split()[0]), payload])
    return pairs


class Mcp:
    def __init__(self, binary, root, env):
        self.process = subprocess.Popen([binary, "mcp", f"--allow-dir={root}"],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, text=True, encoding="utf-8", env=env)
        self.responses = queue.Queue()
        self.sequence = 0
        def reader():
            for line in self.process.stdout:
                self.responses.put(json.loads(line))
        threading.Thread(target=reader, daemon=True).start()
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                   "clientInfo": {"name": "subtitle-fidelity", "version": "1"}})
        self.process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        self.process.stdin.flush()

    def request(self, method, params):
        self.sequence += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.sequence,
                                            "method": method, "params": params}) + "\n")
        self.process.stdin.flush()
        while True:
            response = self.responses.get(timeout=60)
            if response.get("id") == self.sequence:
                assert "error" not in response, response
                result = response["result"]
                assert not result.get("isError"), result
                return result

    def read(self, source):
        result = self.request("tools/call", {"name": "read", "arguments": {
            "source": str(source), "ocr": False, "images": "none", "max_tokens": 10000}})
        return "\n".join(item.get("text", "") for item in result["content"])

    def close(self):
        self.process.kill()
        self.process.wait(timeout=5)


def generated_inputs(root):
    base = root / "base.mp4"
    run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i",
         "color=c=black:s=16x16:r=1:d=1", "-c:v", "mpeg4", base])
    for name in ["sources", "embedded", "srt", "vtt"]:
        (root / name).mkdir()
    inputs = []
    for name, cues in CASES.items():
        expected = [[start, cleaned] for start, _, _, cleaned, _ in cues]
        srt = "\n\n".join(f"{i}\n{clock(start, ',')} --> {clock(end, ',')}\n{text}"
                          for i, (start, end, text, _, _) in enumerate(cues, 1)) + "\n"
        vtt = "WEBVTT\n\n" + "\n\n".join(
            f"{clock(start)} --> {clock(end)}\n{voice or text}"
            for start, end, text, _, voice in cues) + "\n"
        source = root / "sources" / f"{name}.srt"
        source.write_text(srt, encoding="utf-8")
        for extension, codec in [("mp4", "mov_text"), ("mkv", "srt")]:
            media = root / "embedded" / f"{name}.{extension}"
            run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", base,
                 "-i", source, "-map", "0:v", "-map", "1:s", "-c:v", "copy", "-c:s", codec, media])
            reference = ffmpeg_pairs(media)
            assert reference == expected, (media, reference, expected)
            inputs.append((media, expected, reference, "generated-embedded"))
        for extension, text in [("srt", srt), ("vtt", vtt)]:
            media = root / extension / f"{name}.mp4"
            shutil.copyfile(base, media)
            sidecar = media.with_suffix(f".{extension}")
            sidecar.write_text(text, encoding="utf-8")
            inputs.extend([(media, expected, None, f"generated-{extension}-sidecar"),
                           (sidecar, expected, None, f"generated-{extension}-direct")])
    # Test hour-format starts without depending on FFmpeg's handling of a huge
    # leading empty mov_text sample. These are separate, new sidecar fixtures.
    for extension, timing in [("srt", "01:02:03,456 --> 01:02:04,000"),
                              ("vtt", "01:02:03.456 --> 01:02:04.000")]:
        media = root / extension / "hours.mp4"
        shutil.copyfile(base, media)
        media.with_suffix(f".{extension}").write_text(
            ("WEBVTT\n\n" if extension == "vtt" else "1\n") + timing + "\nLate\n", encoding="utf-8")
        inputs.append((media, [[3_723_456, "Late"]], None, f"generated-{extension}-hour-sidecar"))
    for extension in ["mp4", "mkv"]:
        offset = root / f"offset.{extension}"
        run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i",
             root / "embedded" / f"repeated.{extension}", "-map", "0", "-c", "copy",
             "-output_ts_offset", "5", offset])
        expected = [[5000, "Again"], [6000, "Again"]]
        reference = ffmpeg_pairs(offset)
        assert reference == expected, (offset, reference, expected)
        inputs.append((offset, expected, reference, "generated-nonzero-source-clock"))
    return inputs


def historical_inputs(root, repo, existing):
    corpus = json.loads((repo / "bench/media/corpus.json").read_text(encoding="utf-8"))
    spec = importlib.util.spec_from_file_location("media_benchmark", repo / "bench/media/run.py")
    benchmark = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(benchmark)
    historical = root / "historical"
    historical.mkdir()
    inputs = []
    for dataset in corpus["datasets"]:
        for case in corpus["cases"]:
            stem = f"{dataset['id']}-{case['id']}"
            media = historical / f"{stem}.{dataset['extension']}"
            if existing:
                originals = list(existing.resolve().rglob(media.name))
                assert len(originals) == 1, (media.name, originals)
                # Exclude colocated sidecars without modifying the originals.
                shutil.copyfile(originals[0], media)
            else:
                subtitle = historical / f"source-{stem}.srt"
                subtitle.write_text(benchmark.make_srt(case["cues"]), encoding="utf-8")
                # Same generation command as the original benchmark harness.
                run(["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i",
                     "color=c=black:s=160x90:r=10:d=3", "-i", subtitle,
                     "-map", "0:v:0", "-map", "1:s:0", "-c:v", "mpeg4",
                     "-c:s", dataset["subtitle_codec"], "-t", "3", media])
            expected = [[index * 1000, benchmark.normalize(text)] for index, text in enumerate(case["cues"])]
            reference = ffmpeg_pairs(media)
            assert reference == expected, (media, reference, expected)
            inputs.append((media, expected, reference, "original-corpus"))
    assert len(inputs) == 8 and sum(len(expected) for _, expected, _, _ in inputs) == 24
    return inputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, help="Existing original clips (otherwise regenerated from bench/media/corpus.json)")
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    repo = Path(__file__).resolve().parents[1]
    receipt = {"measured_at": datetime.now(timezone.utc).isoformat(),
               "source_commit": run(["git", "rev-parse", "HEAD"], cwd=repo).strip(),
               "source_dirty": bool(run(["git", "status", "--porcelain"], cwd=repo).strip()),
               "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
               "binary_version": run([binary, "version"]).strip(),
               "original_corpus_sha256": hashlib.sha256((repo / "bench/media/corpus.json").read_bytes()).hexdigest(),
               "original_inputs": "supplied-container-bytes" if args.corpus else "regenerated-original-authored-corpus",
               "ffmpeg_version": run(["ffmpeg", "-version"]).splitlines()[0], "checks": []}
    with tempfile.TemporaryDirectory(prefix="subtitle-fidelity-") as directory:
        root = Path(directory)
        env = {key: value for key, value in os.environ.items() if not key.startswith(
            ("ANYMD_", "MCP_", "PDF_READER_", "OPENAI_", "ANTHROPIC_"))}
        env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), ANYMD_CACHE_DIR=str(root / "cache"))
        inputs = generated_inputs(root)
        inputs.extend(historical_inputs(root, repo, args.corpus))
        client = Mcp(binary, root, env)
        try:
            for source, expected, reference, kind in inputs:
                cli = run([binary, source, "--no-ocr", "--images", "none", "--max-tokens", "10000"], env=env)
                mcp = client.read(source)
                assert markdown_pairs(cli) == expected, (source, "CLI", cli, expected)
                assert markdown_pairs(mcp) == expected, (source, "MCP", mcp, expected)
                receipt["checks"].append({"name": source.name, "kind": kind,
                                          "input_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                                          "expected": expected, "ffmpeg": reference,
                                          "cli": markdown_pairs(cli), "mcp": markdown_pairs(mcp)})
            for extension, text, expected in [
                ("srt", "  ordinary text\nnext line  ", "ordinary text\nnext line"),
                ("vtt", "WEBVTT\n\n00:00.000 --> 00:01.000\n<i></i>\n", ""),
                ("txt", "Plain local text", "Plain local text"),
                ("html", "<html><body><p>Local HTML</p></body></html>", "Local HTML"),
                ("csv", "Name,Value\nlocal,42\n", "42"),
            ]:
                source = root / f"fallback.{extension}"
                source.write_text(text, encoding="utf-8")
                for output in [run([binary, source, "--no-ocr", "--images", "none"], env=env), client.read(source)]:
                    assert expected in output and not markdown_pairs(output), (source, output)
                    if extension == "vtt":
                        assert "-->" not in output and "<i>" not in output, output
            assert not list((root / "home").rglob("*.gguf")), "Unexpected model download"
            assert not list((root / "cache").rglob("*.gguf")), "Unexpected model download"
        finally:
            client.close()
    receipt["original_corpus_summary"] = {"clips": 8, "pairs_per_container": 12, "extra_cues": 0}
    receipt["generated_regression_summary"] = {"cases": 8, "pairs_per_container": 12,
                                               "additional_hour_and_offset_inputs": 4}
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Subtitle fidelity passed: {len(receipt['checks'])} CLI/MCP checks; receipt: {args.receipt}")


if __name__ == "__main__":
    main()
