"""Offline benchmark outcome tests: python3 -m unittest discover -s bench -p test_validity.py."""
import contextlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load("agent_runner", "bench/run.py")
merge = load("agent_merge", "bench/merge.py")
omni = load("omni_harness", "bench/omnidocbench/harness.py")
asr = load("asr_runner", "bench-asr/run_engine.py")
report = load("asr_report", "bench-asr/report.py")
plan = load("asr_plan", "bench-asr/plan.py")


class AgentValidityTests(unittest.TestCase):
    def test_failed_conversion_and_unsupported_capability(self):
        for status, expected in [("error", 1), ("missing", 1), ("timeout", 1), ("unsupported", 0), ("ok", 0)]:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "corpus.json").write_text(json.dumps({"docs": [{"id": "a", "category": "pdf", "format": "pdf", "file": "a.pdf"}]}))
                if status != "missing":
                    (root / "a.pdf").write_text("fixture")
                adapter = types.SimpleNamespace(RUNS=1, FORMATS=[] if status == "unsupported" else None, version=lambda: "fixture")
                tokenizer = types.SimpleNamespace(get_encoding=lambda _: types.SimpleNamespace(encode=lambda *a, **kw: []))
                failure = subprocess.TimeoutExpired("fixture", 1) if status == "timeout" else RuntimeError("failed")
                argv = ["run.py", "--tool", "fixture", "--corpus", tmp, "--out", str(root / "out.json")]
                with patch.object(runner, "HERE", root), patch.object(runner, "load_adapter", return_value=adapter), \
                        patch.object(runner, "convert", side_effect=None if status == "ok" else failure, return_value=(0.1, "text")), \
                        patch.object(runner.scoring, "score", return_value={"score": 1}), patch.dict(sys.modules, {"tiktoken": tokenizer}), \
                        patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(runner.main(), expected)
                data = json.loads((root / "out.json").read_text())
                self.assertEqual(data["results"][0]["status"], status)
                self.assertEqual(data["meta"]["expected_ids"], ["a"])

    def test_file_export_adapters_through_runner(self):
        for tool in ["docling", "marker"]:
            adapter = load(f"export_{tool}", f"bench/adapters/{tool}.py")
            for present in [False, True]:
                with self.subTest(tool=tool, present=present), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    (root / "corpus.json").write_text(json.dumps({"docs": [{"id": "a", "category": "pdf", "format": "pdf", "file": "a.pdf"}]}))
                    (root / "a.pdf").write_text("fixture")
                    def process(cmd, **kwargs):
                        folder = Path(cmd[cmd.index("--output" if tool == "docling" else "--output_dir") + 1])
                        if present:
                            (folder / "a.md").write_text("")
                        return types.SimpleNamespace(returncode=0, stdout=b"", stderr=b"")
                    tokenizer = types.SimpleNamespace(get_encoding=lambda _: types.SimpleNamespace(encode=lambda *a, **kw: []))
                    with patch.object(runner, "HERE", root), patch.object(runner, "load_adapter", return_value=adapter), \
                            patch.object(adapter, "version", return_value="fixture"), patch.object(runner.subprocess, "run", side_effect=process), \
                            patch.object(runner.scoring, "score", return_value={"score": 0}) as scoring, \
                            patch.dict(sys.modules, {"tiktoken": tokenizer}), \
                            patch.object(sys, "argv", ["run.py", "--tool", tool, "--corpus", tmp, "--out", str(root / "out.json")]), \
                            contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                        self.assertEqual(runner.main(), 0 if present else 1)
                    row = json.loads((root / "out.json").read_text())["results"][0]
                    self.assertEqual(row["status"], "ok" if present else "error")
                    if present:
                        self.assertEqual(row["bytes"], 0)
                        scoring.assert_called_once_with("a", "")
                    else:
                        self.assertIn("expected one Markdown export, found 0", row["error"])
                        scoring.assert_not_called()

    def parts(self):
        return [{"meta": {"planned_ids": ["a", "b"], "expected_ids": [name], "shard": f"{i}/2", "status": "ok", "tool": "fixture"},
                 "results": [{"doc": name, "status": "ok"}]} for i, name in enumerate(["a", "b"], 1)]

    def test_merge_freezes_ids_and_shards(self):
        self.assertEqual(len(merge.merge(self.parts(), ["a", "b"])["results"]), 2)
        for change in ["missing-shard", "duplicate-shard", "missing-row", "duplicate-row", "unexpected-row", "wrong-plan"]:
            parts = self.parts()
            if change == "missing-shard": parts.pop()
            if change == "duplicate-shard": parts.append(parts[0])
            if change == "missing-row": parts[0]["results"] = []
            if change == "duplicate-row": parts[0]["results"] *= 2
            if change == "unexpected-row": parts[0]["results"][0]["doc"] = "other"
            if change == "wrong-plan": parts[0]["meta"]["planned_ids"] = ["a"]
            with self.subTest(change=change), self.assertRaises(ValueError):
                merge.merge(parts, ["a", "b"])

    def test_merge_retains_failed_diagnostics_and_optional_exclusions(self):
        parts = self.parts()
        parts[0]["results"][0]["status"] = "unsupported"
        self.assertEqual(merge.merge(parts, ["a", "b"])["meta"]["status"], "ok")
        parts[1]["results"][0]["status"] = "error"
        self.assertEqual(merge.merge(parts, ["a", "b"])["meta"]["status"], "failed")

    def test_selection_rejects_unknown_duplicate_and_invalid_shards(self):
        for docs, selected, shard in [([{"id": "a"}], {"absent"}, ""), ([{"id": "a"}] * 2, set(), ""), ([{"id": "a"}], set(), "0/2")]:
            with self.assertRaises(ValueError): runner.select(docs, selected, shard)

    def test_workflow_producer_pipeline_checks_status_and_current_results(self):
        workflow = (ROOT / ".github/workflows/benchmark.yml").read_text()
        block = workflow.split("- name: Merge shards and render", 1)[1]
        self.assertIn("set -euo pipefail", block)
        self.assertIn("--results current-results", block)
        self.assertNotIn("for tool in $(ls", block)
        result = subprocess.run(["bash", "-c", "set -euo pipefail; python3 -c 'raise SystemExit(7)' | cat"], capture_output=True)
        self.assertEqual(result.returncode, 7)


class OmniValidityTests(unittest.TestCase):
    def setup_parts(self, root):
        gt = root / "gt.json"
        gt.write_text(json.dumps([{"page_info": {"image_path": "a.jpg"}}, {"page_info": {"image_path": "b.jpg"}}]))
        parts = root / "parts"
        parts.mkdir()
        for i, name in enumerate(["a", "b"], 1):
            folder = parts / str(i)
            folder.mkdir()
            prediction = folder / f"{name}.md"
            prediction.write_text("text")
            (folder / f"timings-{i}of2.json").write_text(json.dumps({"shard": f"{i}/2", "revision": omni.REVISION,
                "planned_pages": ["a.jpg", "b.jpg"], "expected_pages": [f"{name}.jpg"],
                "pages": {f"{name}.jpg": {"ok": True, "chars": 4, "sha256": omni.sha256(prediction)}}}))
        return types.SimpleNamespace(gt=str(gt), parts=str(parts))

    def test_validation_rejects_failed_missing_duplicate_and_modified_predictions(self):
        for change in [None, "failed", "missing-timing", "missing-file", "duplicate-file", "changed-file", "missing-row"]:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                args = self.setup_parts(root)
                timing = root / "parts/1/timings-1of2.json"
                data = json.loads(timing.read_text())
                if change == "failed": data["pages"]["a.jpg"]["ok"] = False; timing.write_text(json.dumps(data))
                if change == "missing-timing": timing.unlink()
                if change == "missing-file": (root / "parts/1/a.md").unlink()
                if change == "duplicate-file": (root / "parts/2/a.md").write_text("text")
                if change == "changed-file": (root / "parts/1/a.md").write_text("wrong")
                if change == "missing-row": data["pages"] = {}; timing.write_text(json.dumps(data))
                with contextlib.redirect_stdout(io.StringIO()):
                    if change is None: self.assertEqual(omni.validate(args), 0)
                    else:
                        with self.assertRaises(ValueError): omni.validate(args)

    def test_converter_requires_success_and_ocr_contract_not_nonempty_transcript(self):
        for code, text, valid in [(1, "", False), (0, "metadata only", False), (0, "## Text (OCR)\n_no text_", True)]:
            with self.subTest(code=code, text=text), patch.object(omni.subprocess, "run", return_value=types.SimpleNamespace(returncode=code, stdout=text, stderr="failed")):
                self.assertEqual(omni.convert("fixture", Path("a.jpg"), 1)[0], valid)

    def test_predict_records_failures_before_nonzero(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "images").mkdir()
            (root / "images/a.jpg").write_text("image")
            args = types.SimpleNamespace(cache=tmp, limit=0, shard="1/1", out=str(root / "out"), workers=1, timeout=1)
            with patch.object(omni, "annotations", return_value=(None, [{"page_info": {"image_path": "a.jpg"}}])), \
                    patch.object(omni, "manifest", return_value={"images/a.jpg": omni.sha256(root / "images/a.jpg")}), \
                    patch.object(omni, "convert", return_value=(False, "", 0.1, "failed")), \
                    patch.object(omni.subprocess, "run", return_value=types.SimpleNamespace(stdout="fixture")), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(omni.predict(args), 1)
            timing = json.loads((root / "out/timings-1of1.json").read_text())
            self.assertFalse(timing["pages"]["a.jpg"]["ok"])
            self.assertEqual(timing["expected_pages"], ["a.jpg"])


class AsrValidityTests(unittest.TestCase):
    def test_sherpa_exact_paths_banner_missing_duplicate_and_empty(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            wav = root / "a.wav"
            args = types.SimpleNamespace(tool_dir=tmp, model=tmp, threads=1, lang_mode="explicit", timeout=1)
            fixtures = [
                (f"sherpa --num-threads=1 {wav}\n/other.wav\n{wav}\n" + '{"text":"hello"}', "hello", False),
                (f"{wav}\n" + '{"text":""}', "", False),
                ("", None, False),
                (f"{wav}\n", None, True),
                (f"{wav}\n{wav}\n" + '{"text":"a"}\n{"text":"b"}', None, True),
                ('/other.wav\n{"text":"hello"}', None, True),
            ]
            for output, expected, invalid in fixtures:
                with self.subTest(output=output), patch.object(asr, "find_exe", return_value=root / "fixture"), \
                        patch.object(asr, "lib_env", return_value={}), \
                        patch.object(asr.subprocess, "run", return_value=types.SimpleNamespace(returncode=0, stdout="", stderr=output)) as process:
                    if invalid:
                        with self.assertRaises(ValueError): asr.run("sherpa-onnx", args, [wav], "en", root)
                    else:
                        rows = asr.run("sherpa-onnx", args, [wav], "en", root)[0]
                        if expected is None: self.assertEqual(rows, {})
                        else: self.assertEqual(rows[wav][0], expected)
                    self.assertIn("--print-args=false", process.call_args.args[0])

    def test_report_consumes_actual_planner_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "output"
            with patch.dict(plan.os.environ, {"GITHUB_OUTPUT": str(output), "ENGINES": "sherpa-onnx", "MACOS_UTTS": "0", "EVENT": "workflow_dispatch"}), \
                    contextlib.redirect_stdout(io.StringIO()):
                plan.main()
            matrix = json.loads(dict(line.split("=", 1) for line in output.read_text().splitlines())["matrix"])
            jobs = [({"job_id": entry["id"]}, []) for entry in matrix]
            self.assertEqual(report.validate_plan(matrix, jobs), [])
            self.assertEqual(len(report.validate_plan(matrix, jobs[:-1])), 1)
            self.assertTrue(all(entry["engine"] == "sherpa-onnx" for entry in matrix))

    def test_selected_job_coverage_and_report_exit(self):
        summary = {"job_id": "selected", "status": "ok", "label": "fixture", "os": "Linux", "dataset": "fleurs-en",
                   "expected_ids": ["a"], "n": 1, "rtf_incl": 1, "audio_s": 1, "rss_mb": 1, "cpu": "fixture",
                   "cpus": 1, "exe_bytes": 1, "libs_bytes": 0}
        row = {"id": "a", "status": "ok", "ref": "hello", "hyp": "", "ts": "none"}
        for case in ["complete", "absent", "duplicate", "unexpected", "setup-failed"]:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "plan.json").write_text(json.dumps([{"id": "selected"}]))
                current = dict(summary)
                if case == "unexpected": current["job_id"] = "unselected"
                if case == "setup-failed": current.update(status="failed", error="setup failed")
                if case != "absent":
                    for i in range(2 if case == "duplicate" else 1):
                        folder = root / str(i)
                        folder.mkdir()
                        (folder / "summary.json").write_text(json.dumps(current))
                        (folder / "results.jsonl").write_text(json.dumps(row) + "\n")
                with patch.object(sys, "argv", ["report.py", tmp, "--plan", str(root / "plan.json")]), \
                        patch.object(report, "load_normalizers", return_value=(str.lower, str.lower)), \
                        patch.object(report, "score", return_value=(100, 100, 100, "word")), \
                        contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(report.main(), 0 if case == "complete" else 1)
                if case == "absent": self.assertIn("selected job selected: expected one outcome, found 0", output.getvalue())
                if case == "setup-failed": self.assertIn("setup failed", output.getvalue())
        historical = dict(summary)
        historical.pop("expected_ids")
        historical.pop("job_id")
        historical_row = dict(row)
        historical_row.pop("status")
        report.validate_job(historical, [historical_row], allow_legacy=True)
        with self.assertRaises(ValueError): report.validate_job(historical, [historical_row])
        with self.assertRaises(ValueError): report.validate_job(dict(historical, n=2), [historical_row], allow_legacy=True)
        # An engine not selected by plan.py needs no artifact.
        self.assertEqual(report.validate_plan([{"id": "selected"}], [(summary, [row])]), [])
        self.assertTrue(report.validate_plan([{"id": "selected"}] * 2, [(summary, [row])]))


    def test_missing_artifact_is_not_empty_transcript(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            args = types.SimpleNamespace(tool_dir=tmp, model="fixture", threads=1, lang_mode="explicit", timeout=1, aligner=None)
            for engine in ["whisper", "crispasr"]:
                for artifact in [False, True]:
                    wav = root / "a.wav"
                    def process(*a, **kw):
                        if artifact:
                            output = wav.with_name(wav.name + ".json") if engine == "whisper" else wav.with_suffix(".json")
                            output.write_text('{"transcription": []}')
                        return types.SimpleNamespace(returncode=0, stdout="", stderr="")
                    with patch.object(asr, "find_exe", return_value=root / "fixture"), patch.object(asr, "lib_env", return_value={}), patch.object(asr.subprocess, "run", side_effect=process):
                        rows = asr.run(engine, args, [wav], "en", root)[0]
                    self.assertEqual(wav in rows, artifact)
                    if artifact: self.assertEqual(rows[wav][0], "")

    def test_partial_batch_fails_and_removes_full_audio_throughput(self):
        for complete in [False, True]:
            with self.subTest(complete=complete), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                items = [{"id": str(i), "wav": f"{i}.wav", "lang": "en", "dataset": "fleurs-en", "dur": i + 1, "ref": "hello"} for i in range(2)]
                manifest = root / "manifest.jsonl"
                manifest.write_text("".join(json.dumps(i) + "\n" for i in items))
                def batch(engine, args, wavs, *rest):
                    chosen = wavs if complete else wavs[:1]
                    return {w: ("", "none", []) for w in chosen}, None, 0.1 * len(wavs), "fixture"
                argv = ["run_engine.py", "transcribe-cpp", "--manifest", str(manifest), "--out", str(root / "out"), "--tool-dir", tmp, "--model", "fixture"]
                with patch.object(asr, "find_exe", return_value=root / "fixture"), patch.object(asr, "footprint", return_value={}), \
                        patch.object(asr, "cpu_model", return_value="fixture"), patch.object(asr, "rss_mb", return_value=1), \
                        patch.object(asr, "run", side_effect=batch), patch.object(sys, "argv", argv), \
                        contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(asr.main(), 0 if complete else 1)
                summary = json.loads((root / "out/summary.json").read_text())
                rows = [json.loads(l) for l in (root / "out/results.jsonl").read_text().splitlines()]
                if complete: report.validate_job(summary, rows)
                else:
                    self.assertIsNone(summary["rtf_incl"])
                    self.assertIsNone(summary["rtf_excl"])
                    self.assertEqual(summary["status"], "failed")
                    with self.assertRaises(ValueError): report.validate_job(summary, rows)

    def test_report_exits_nonzero_for_absent_or_incomplete_success_claim(self):
        for present in [False, True]:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                if present:
                    (root / "summary.json").write_text(json.dumps({"status": "ok", "label": "fixture", "os": "Linux",
                        "dataset": "fleurs-en", "expected_ids": ["a", "b"], "n": 2, "rtf_incl": 1}))
                    (root / "results.jsonl").write_text(json.dumps({"id": "a", "status": "ok"}) + "\n")
                with patch.object(sys, "argv", ["report.py", tmp]), \
                        patch.object(report, "load_normalizers", return_value=(str.lower, str.lower)), \
                        contextlib.redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(report.main(), 1)
                self.assertNotIn("| fixture | Linux | fleurs-en |", output.getvalue())

    def test_report_rejects_missing_duplicate_and_failed_rows(self):
        summary = {"expected_ids": ["a", "b"], "n": 2, "missing": 0, "rtf_incl": 1}
        for rows in [[], [{"id": "a", "status": "ok"}] * 2, [{"id": "a", "status": "ok"}, {"id": "b", "status": "missing"}]]:
            with self.assertRaises(ValueError): report.validate_job(summary, rows)
        report.validate_job(summary, [{"id": n, "status": "ok", "hyp": ""} for n in ["a", "b"]])


if __name__ == "__main__":
    unittest.main()
