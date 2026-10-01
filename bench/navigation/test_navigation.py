import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import run

ROOT = Path(__file__).resolve().parent


class NavigationTests(unittest.TestCase):
    def setUp(self):
        self.manifest = run.load_manifest(ROOT / "fixtures/manifest.json")
        self.question = self.manifest["questions"][0]

    def test_correct_answer_wrong_page_fails_joint_score(self):
        result = run.score(self.question, {"answer": " orbit ", "pages": [2]})
        self.assertTrue(result["answer_exact"])
        self.assertFalse(result["answer_and_source_page"])

    def test_missing_and_overcitation(self):
        self.assertFalse(run.score(self.question, {"answer": "ORBIT"})["source_page_hit"])
        result = run.score(self.question, {"answer": "ORBIT", "pages": [1, 2]})
        self.assertEqual(result["source_page_precision"], 0.5)
        self.assertEqual(result["source_page_recall"], 1)

    def test_page_numbers_are_not_zero_based(self):
        for page in (0, -1, "1", True):
            with self.assertRaises(ValueError):
                run.score(self.question, {"answer": "ORBIT", "pages": [page]})

    def test_unanswered_rows_remain_in_denominator(self):
        result = run.evaluate(self.manifest, [{"id": "launch", "answer": "ORBIT", "pages": [1]}])
        self.assertEqual(result["answer_source_page_accuracy"], 0.5)
        self.assertEqual(result["rows"][1]["prediction"]["status"], "missing")

    def test_unknown_duplicate_prediction_rejected(self):
        for predictions in ([{"id": "unknown"}], [{"id": "launch"}, {"id": "launch"}]):
            with self.assertRaises(ValueError):
                run.evaluate(self.manifest, predictions)

    def test_hash_mismatch_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "navigation.pdf").write_bytes(b"changed")
            (root / "manifest.json").write_text(json.dumps(self.manifest))
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                run.load_manifest(root / "manifest.json")

    def test_document_path_cannot_escape(self):
        self.manifest["documents"][0]["file"] = "../outside.pdf"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps(self.manifest))
            with self.assertRaisesRegex(ValueError, "escapes"):
                run.load_manifest(path)

    def test_pageindex_documented_responses_contract(self):
        envelope = json.loads((ROOT / "fixtures/pageindex-response.json").read_text())
        prediction = run.pageindex_response(envelope)
        self.assertEqual(prediction["answer"], "ORBIT")
        self.assertEqual(prediction["pages"], [1])
        self.assertEqual(prediction["retrieval_calls"], 1)
        self.assertEqual(prediction["input_tokens"], 100)
        self.assertTrue(run.score(self.question, prediction)["answer_and_source_page"])
        del envelope["usage"]
        envelope["output"][0]["content"][0]["text"] = "ORBIT"
        unknown = run.pageindex_response(envelope)
        self.assertIsNone(unknown["input_tokens"])
        self.assertEqual(unknown["pages"], [])

    def test_merged_anymd_outline_node_search_read_contract(self):
        responses = ["anymd contract-test", json.dumps({"nodes": [
            {"id": "n1.1", "title": "1. Launch", "from": 1, "to": 1}]}),
            '1 match\n- p.1: The launch code is ORBIT.',
            '<!-- page 1 -->\nThe launch code is ORBIT.']
        def invoke(argv, **kwargs):
            return subprocess.CompletedProcess(argv, 0, responses.pop(0), "")
        with patch("run.subprocess.run", side_effect=invoke) as mock:
            tool = run.Anymd("/example/anymd")
            prediction = tool.ask("fixture.pdf", self.question, "outline-node")
        self.assertEqual([c["operation"] for c in tool.calls], ["outline", "search", "read"])
        self.assertIn("--node", mock.call_args.args[0])
        self.assertIn("n1.1", mock.call_args.args[0])
        self.assertTrue(run.score(self.question, prediction)["answer_and_source_page"])

    def test_nonzero_tool_exit_is_not_a_success(self):
        with patch("run.subprocess.run", return_value=subprocess.CompletedProcess([], 2, "", "unsupported")):
            tool = run.Anymd.__new__(run.Anymd)
            tool.binary, tool.calls = "anymd", []
            with self.assertRaises(RuntimeError):
                tool.outline("fixture.pdf")
        self.assertEqual(tool.calls[0]["exit_code"], 2)


if __name__ == "__main__":
    unittest.main()
