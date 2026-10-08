import hashlib
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("media_run", Path(__file__).with_name("run.py"))
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


class ScoringTests(unittest.TestCase):
    def test_source_srt_roundtrip(self):
        self.assertEqual(run.srt_cues(run.make_srt(["First\nline", "第二句"])),
                         [(0, "First line"), (1, "第二句")])

    def test_markdown_hour_and_minute_timestamps(self):
        self.assertEqual(run.markdown_cues("## subtitles\n[00:01] Hello\n[01:02:03] 世界"),
                         [(1, "Hello"), (3723, "世界")])

    def test_dropped_repeated_cue_is_not_credited(self):
        expected = [(0, "Again"), (1, "Again")]
        self.assertEqual(run.score(expected, [(0, "Again")]),
                         {"matched": 1, "expected": 2, "extracted": 1})

    def test_duplicate_output_cannot_inflate_matches(self):
        self.assertEqual(run.score([(0, "Once")], [(0, "Once"), (0, "Once")])["matched"], 1)

    def test_wrong_timestamp_or_text_is_not_a_match(self):
        self.assertEqual(run.score([(0, "Correct")], [(1, "Correct"), (0, "Wrong")])["matched"], 0)

    def test_empty_output_scores_zero(self):
        self.assertEqual(run.score([(0, "Expected")], [])["matched"], 0)


class PublishedResultsTests(unittest.TestCase):
    def test_saved_outputs_reproduce_counts_and_tables(self):
        directory = Path(__file__).parent
        corpus_bytes = (directory / "corpus.json").read_bytes()
        corpus = json.loads(corpus_bytes)
        results = json.loads((directory / "results.json").read_text())
        self.assertEqual(results["corpus_sha256"], hashlib.sha256(corpus_bytes).hexdigest())
        cases = {case["id"]: case["cues"] for case in corpus["cases"]}
        expected_keys = {(dataset["id"], case, tool) for dataset in corpus["datasets"]
                         for case in cases for tool in ("anymd", "ffmpeg")}
        keys = [(row["dataset"], row["case"], row["tool"]) for row in results["rows"]]
        self.assertEqual(len(keys), len(expected_keys))
        self.assertEqual(set(keys), expected_keys)
        for row in results["rows"]:
            self.assertEqual(row["exit_code"], 0)
            expected = [(index, run.normalize(text)) for index, text in enumerate(cases[row["case"]])]
            parse = run.markdown_cues if row["tool"] == "anymd" else run.srt_cues
            counts = run.score(expected, parse(row["output"]))
            self.assertEqual(counts, {key: row[key] for key in counts})
        guide = (directory.parents[1] / "docs/guide/benchmarks.md").read_text()
        method = (directory / "README.md").read_text()
        for dataset in corpus["datasets"]:
            for tool in ("anymd", "ffmpeg"):
                rows = [row for row in results["rows"]
                        if row["dataset"] == dataset["id"] and row["tool"] == tool]
                matches = sum(row["matched"] for row in rows)
                expected = sum(row["expected"] for row in rows)
                self.assertIn(f"{matches}/{expected}", guide)
                self.assertIn(f"{matches}/{expected}", method)
                seconds = sum(row["seconds"] for row in rows)
                self.assertIn(f"{seconds:.3f} s", guide)
                self.assertIn(f"{seconds:.3f} s", method)


if __name__ == "__main__":
    unittest.main()
