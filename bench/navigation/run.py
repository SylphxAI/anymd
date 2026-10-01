"""Keyless navigation smoke and offline QA scoring. No model/network calls."""
import argparse
import hashlib
import json
import re
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def load_manifest(path):
    manifest = json.loads(path.read_text())
    for doc in manifest["documents"]:
        source = (path.parent / doc["file"]).resolve()
        if not source.is_relative_to(path.parent.resolve()):
            raise ValueError("document escapes corpus directory")
        if hashlib.sha256(source.read_bytes()).hexdigest() != doc["sha256"]:
            raise ValueError(f"document hash mismatch: {doc['id']}")
    ids = [q["id"] for q in manifest["questions"]]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate question id")
    return manifest


def normalize(text):
    return " ".join(text.casefold().split())


def score(question, prediction):
    """Exact answer/page scoring; explicit error/missing rows always score zero."""
    if "status" in prediction:
        status = prediction["status"]
        if status not in ("ok", "error", "missing"):
            raise ValueError("prediction status must be ok, error, or missing")
        if status != "ok":
            return {"answer_exact": False, "source_page_hit": False,
                    "source_page_precision": 0.0, "source_page_recall": 0.0,
                    "answer_and_source_page": False}
    answer_ok = normalize(prediction.get("answer", "")) in {
        normalize(a) for a in question["answers"]
    }
    expected = set(question["evidence_pages"])
    cited = set(prediction.get("pages", []))
    if any(type(p) is not int or p < 1 for p in cited):
        raise ValueError("citations must be positive integer physical pages")
    overlap = expected & cited
    page_precision = len(overlap) / len(cited) if cited else 0.0
    page_recall = len(overlap) / len(expected) if expected else 0.0
    return {"answer_exact": answer_ok, "source_page_hit": bool(overlap),
            "source_page_precision": page_precision, "source_page_recall": page_recall,
            "answer_and_source_page": answer_ok and bool(overlap)}


class Anymd:
    """Released/merged CLI contract. Every search/read/outline is metered."""
    def __init__(self, binary):
        self.binary = binary
        self.version = subprocess.run([binary, "version"], check=True, capture_output=True,
                                      text=True, timeout=30).stdout.strip()
        self.calls = []

    def call(self, operation, *args):
        start = time.perf_counter()
        try:
            result = subprocess.run([self.binary, *map(str, args)], capture_output=True,
                                    text=True, timeout=60)
        except subprocess.TimeoutExpired:
            self.calls.append({"operation": operation, "arguments": list(map(str, args)),
                               "seconds": time.perf_counter() - start, "exit_code": None,
                               "output_bytes": None, "output": "", "stderr": "timeout",
                               "status": "timeout; partial output not retained"})
            raise
        elapsed = time.perf_counter() - start
        self.calls.append({"operation": operation, "arguments": list(map(str, args)),
                           "seconds": elapsed, "exit_code": result.returncode,
                           "output_bytes": len(result.stdout.encode()),
                           "output": result.stdout, "stderr": result.stderr})
        if result.returncode:
            raise RuntimeError(f"{operation}: {result.stderr.strip()}")
        return result.stdout

    def outline(self, source):
        return json.loads(self.call("outline", "outline", source, "--format", "json"))

    def search(self, source, query):
        return self.call("search", "search", query, source, "--max", "5")

    def read(self, source, *, node=None, page=None):
        selection = ["--node", node] if node else ["--pages", str(page)]
        return self.call("read", source, *selection, "--no-ocr", "--images", "none",
                         "--max-tokens", "2000")

    def ask(self, source, question, policy):
        if policy == "outline-node":
            nodes = self.outline(source)["nodes"]
            # Fixture-only scripted policy; no reference answers/pages consulted.
            node = next(n for n in nodes if question["section"].casefold() in n["title"].casefold())
            self.search(source, question["query"])
            text = self.read(source, node=node["id"])
            pages = [int(p) for p in re.findall(r"<!-- page (\d+) -->", text)]
        else:
            hits = self.search(source, question["query"])
            match = re.search(r"\bp\.(\d+):", hits)
            if not match:
                return {"answer": "", "pages": []}
            page = int(match[1])
            text = self.read(source, page=page)
            pages = [page]
        answer = re.search(question["pattern"], text)
        return {"answer": answer[1] if answer else "", "pages": sorted(set(pages))}


def pageindex_response(envelope):
    """Adapt the OSS Benchmark's documented Responses dialect, without importing SDK.

    Source citations are explicit '[page N]' in final answer text, not inferred from
    function-call arguments or reference pages. Missing usage stays unknown.
    """
    text = "\n".join(c["text"] for item in envelope["output"]
                     if item.get("type") == "message"
                     for c in item.get("content", []) if c.get("text"))
    pages = [int(p) for p in re.findall(r"\[page (\d+)\]", text)]
    answer = re.sub(r"\s*\[page \d+\]", "", text).strip()
    usage = envelope.get("usage")
    return {"answer": answer, "pages": sorted(set(pages)),
            "retrieval_calls": sum(i.get("type") == "function_call" for i in envelope["items"]),
            "input_tokens": usage.get("input_tokens") if usage else None,
            "output_tokens": usage.get("output_tokens") if usage else None,
            "token_usage": usage}


def evaluate(manifest, predictions):
    by_id = {}
    known = {q["id"] for q in manifest["questions"]}
    for prediction in predictions:
        if prediction["id"] not in known or prediction["id"] in by_id:
            raise ValueError("unknown or duplicate prediction id")
        by_id[prediction["id"]] = prediction
    rows = []
    for question in manifest["questions"]:
        prediction = by_id.get(question["id"], {"answer": "", "pages": [], "status": "missing"})
        rows.append({"id": question["id"], "prediction": prediction,
                     "scores": score(question, prediction)})
    return {"track": manifest["track"], "questions": len(rows), "rows": rows,
            "answer_accuracy": sum(r["scores"]["answer_exact"] for r in rows) / len(rows),
            "answer_source_page_accuracy": sum(r["scores"]["answer_and_source_page"] for r in rows) / len(rows)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=ROOT / "fixtures/manifest.json")
    parser.add_argument("--out", type=Path, required=True)
    sub = parser.add_subparsers(dest="adapter", required=True)
    anymd = sub.add_parser("anymd")
    anymd.add_argument("--binary", required=True)
    anymd.add_argument("--policy", choices=["search-page", "outline-node"], default="outline-node")
    replay = sub.add_parser("replay")
    replay.add_argument("--predictions", type=Path, required=True)
    pageindex = sub.add_parser("pageindex-replay")
    pageindex.add_argument("--responses", type=Path, required=True,
                           help="rows with id, envelope and optional measured_seconds/cost_metadata")
    args = parser.parse_args()
    manifest = load_manifest(args.manifest)
    if args.adapter == "replay":
        predictions = json.loads(args.predictions.read_text())
        metadata = {"adapter": "offline-replay", "inference_executed": False}
    elif args.adapter == "pageindex-replay":
        predictions = []
        for row in json.loads(args.responses.read_text()):
            prediction = pageindex_response(row["envelope"])
            prediction.update({"id": row["id"], "retrieval_seconds": row.get("measured_seconds"),
                               "cost_metadata": row.get("cost_metadata"),
                               "model_metadata": row.get("model_metadata"),
                               "index_metadata": row.get("index_metadata")})
            predictions.append(prediction)
        metadata = {"adapter": "pageindex-oss-responses-replay", "inference_executed": False,
                    "call_metric": "all function_call items, as counted by upstream benchmark"}
    else:
        tool = Anymd(args.binary)
        predictions = []
        sources = {d["id"]: args.manifest.parent / d["file"] for d in manifest["documents"]}
        for question in manifest["questions"]:
            tool.calls = []
            try:
                prediction = tool.ask(sources[question["document"]], question, args.policy)
                prediction["status"] = "ok"
            except (RuntimeError, ValueError, KeyError, StopIteration, subprocess.TimeoutExpired) as error:
                prediction = {"answer": "", "pages": [], "status": "error", "error": str(error)}
            prediction.update({"id": question["id"], "calls": tool.calls,
                               "retrieval_calls": len(tool.calls),
                               "retrieval_seconds": sum(c["seconds"] for c in tool.calls),
                               "retrieval_output_bytes": (sum(c["output_bytes"] for c in tool.calls)
                                                          if all(c["output_bytes"] is not None for c in tool.calls) else None),
                               "input_tokens": None, "output_tokens": None, "inference_cost_usd": 0})
            predictions.append(prediction)
        metadata = {"adapter": "anymd-cli", "version": tool.version, "policy": args.policy,
                    "binary_sha256": hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
                    "inference_executed": False, "token_metric": "unknown; bytes are not tokens"}
    report = evaluate(manifest, predictions)
    report.update({"metadata": metadata, "manifest_sha256": hashlib.sha256(args.manifest.read_bytes()).hexdigest()})
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: report[k] for k in ("track", "questions", "answer_accuracy", "answer_source_page_accuracy")}))
    return 1 if any(p.get("status") == "error" for p in predictions) else 0


if __name__ == "__main__":
    raise SystemExit(main())
