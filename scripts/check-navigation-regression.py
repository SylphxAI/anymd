#!/usr/bin/env python3
"""Default CLI output must stay byte-identical on the full AgentDocBench corpus.

Both binaries run on the same runner, with the same helper tools and image cache.
Byte parity is stronger than score parity: every benchmark scorer receives the
same Markdown, so extraction scores cannot drop because of navigation changes.
"""
import argparse
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True)
    parser.add_argument("--after", required=True)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    docs = json.loads((root / "bench/corpus.json").read_text())["docs"]
    results = []
    for doc in docs:
        source = Path(args.corpus) / doc["file"]
        outputs = []
        for binary in (args.before, args.after):
            result = subprocess.run([binary, str(source)], capture_output=True, timeout=900)
            if result.returncode:
                raise RuntimeError(f"{doc['id']}: conversion failed with exit {result.returncode}")
            outputs.append(result.stdout)
        same = outputs[0] == outputs[1]
        results.append({"document": doc["id"], "bytes": len(outputs[1]), "identical": same})
        print(f"{doc['id']}: {'identical' if same else 'CHANGED'}", flush=True)
    Path(args.report).write_text(json.dumps(results, indent=2) + "\n")
    if not all(item["identical"] for item in results):
        raise SystemExit("Default output changed on AgentDocBench corpus")
    print(f"All {len(results)} documents are byte-identical; AgentDocBench scores cannot drop.")


if __name__ == "__main__":
    main()
