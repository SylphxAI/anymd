# Navigation QA harness

This is a reproducible **contract smoke**, plus offline answer/source-page scoring.
It is separate from AgentDocBench extraction scores. It does not establish which
retrieval system is better, and does not execute inference, import model SDKs,
read credentials, or contact a provider.

## Run without credentials

The required CI code-quality job runs offline navigation discovery alongside the
existing Python API/wheel tests. Python 3.10+ and a supplied anymd binary are
enough for the local smoke. From the repository root:

```sh
python3 -m unittest discover -s bench/navigation -p 'test_*.py' -v
python3 bench/navigation/run.py --out /tmp/navigation.json anymd \
  --binary /absolute/path/to/anymd --policy outline-node
```

`outline-node` exercises the merged CLI contract: outline JSON, search, then node
read with OCR off and image export disabled. anymd 8.2.0 has search/page read but
not outline/node read. For that released binary, use `--policy search-page`.
The harness does not silently fall back; unsupported outline is a recorded error,
zero-scored question, and nonzero process exit. A compiled merged binary belongs
in CI, not a local Rust build for this benchmark. The offline test mocks the
merged contract; it is not proof of running that binary.

The committed two-page PDF, questions and reference answers are original CC0
fixtures. They are synthetic, deliberately simple and available equally to both
systems. `make_fixture.py` reproduces the exact PDF and manifest. SHA-256 checks
freeze document bytes; reports also record the manifest and binary hashes. The
scripted smoke policy uses query/section names and answer patterns, but never
reference answers or reference evidence pages. These fixture-specific patterns
are not an autonomous QA agent or a general question-answering evaluator.

## Adapters and metrics

- **anymd:** the documented CLI's outline JSON (`nodes` with `id` and `title`),
  literal/BM25 search text and node/page read. Report saves all tool arguments,
  stdout, stderr, exit codes, wall seconds and UTF-8 output bytes. A fresh CLI
  process per call includes startup; no warm-up is excluded. Source citations in
  node mode come from returned page comments, not expected evidence pages.
- **PageIndex OSS:** `pageindex_response()` adapts the Responses envelope used by
  the pinned OSS benchmark: `output` message content, `items` function calls and
  `usage` input/output tokens. No cloud SDK tools or invented local APIs. Missing
  tokens, latency and price remain unknown, not zero. The adapter is **offline**
  contract evidence for a separately labelled future native-pipeline track B;
  it does not implement or measure the selected shared-controller track A.
  Live A needs pinned public local tree/page API verification.
  An explicit final-answer convention `ANSWER [page N]` supplies physical
  1-based citations for both systems in a future controlled run. This convention
  is ours, not a claimed native PageIndex citation schema.

To score saved normalized predictions (`id`, `answer`, `pages`, optional measured
metrics), use:

```sh
python3 bench/navigation/run.py --out /tmp/scored.json replay --predictions predictions.json
python3 bench/navigation/run.py --out /tmp/pageindex.json pageindex-replay --responses responses.json
```

`responses.json` is an array of `{ "id": "launch", "envelope": { ... } }` rows;
optional `measured_seconds`, `model_metadata`, `index_metadata`, `cost_metadata`
are preserved, not estimated. The handwritten envelope fixture is explicitly a
contract example, not an inference receipt or a measurement. Replay performs no
inference. Reference answers/pages stay outside agent inputs.

Answer accuracy is normalized exact match against accepted aliases; source-page
accuracy is overlap with the annotated physical pages, with precision and recall
also reported to expose overcitation. Joint answer/source-page accuracy requires
both. Prediction status accepts `ok`, `error`, or `missing`; an absent status
preserves normalized replay compatibility and is scored normally. Explicit `error`
or `missing` rows score zero on every answer/page/joint metric, even if their raw
answer and citations are correct. Raw prediction/error details are retained.
Unsupported statuses are rejected. Missing predictions remain zero in the
denominator; duplicates and unknown IDs are rejected.
Exact match is not a semantic judge, and page overlap does not
prove that cited text entails the answer. Unsupported inputs and failed calls
remain visible rather than being silently removed.

Tool-call counts and retrieval wall time are distinct from model input/output
usage. PageIndex call counts follow upstream's count of **all** `function_call`
items, not a fabricated fine-grained retrieval breakdown. anymd performs no
inference here; model tokens are unknown/not applicable, and byte counts are
**not token counts**. A fair model run needs one pinned tokenizer for both tool
transcripts, separately from provider-reported billable input/output/reasoning/
cache tokens. Indexing wall time, tokens and price must be separate from query
cost; cached trees do not make initial indexing free.

## Exact source verification (2026-10-01)

These are facts from source files, not comparative results:

| Source | Verified contract / rights |
| --- | --- |
| [PageIndex MIT licence](https://github.com/VectifyAI/PageIndex/blob/f279431eb4e47884862961b9718df180552f417a/LICENSE) | MIT applies to the software, not automatically to benchmark PDFs. |
| [PageIndex README](https://github.com/VectifyAI/PageIndex/blob/f279431eb4e47884862961b9718df180552f417a/README.md) | Local text-PDF indexing; `PageIndexClient(index=..., chat=...)`, `submit_document`, `chat`; provider key required in the quickstart. |
| [OSS Benchmark README](https://github.com/VectifyAI/PageIndex-OSS-Benchmark/blob/ad4c0b92970a6f4801f09ff2e647389e8f5874fa/README.md) | 62 lookup questions / 34 text PDFs; excludes charts/tables/arithmetic and PDFs flash cannot index. `pageindex>=0.2.10.dev4`; indexing model separate from chat model. |
| [OSS runner](https://github.com/VectifyAI/PageIndex-OSS-Benchmark/blob/ad4c0b92970a6f4801f09ff2e647389e8f5874fa/run_variant.py) / [helpers](https://github.com/VectifyAI/PageIndex-OSS-Benchmark/blob/ad4c0b92970a6f4801f09ff2e647389e8f5874fa/pi_bench.py) | Runner uses `index_model` / `chat_model` and `responses(question, doc_id=..., reasoning=..., max_turns=...)`; helper submits with `wait=True, mode="flash"`. This differs from README's quickstart; pin and verify the installed SDK before live use. Index metering in upstream wraps private globals, not a public SDK contract. |
| [OSS Benchmark root LICENSE request](https://raw.githubusercontent.com/VectifyAI/PageIndex-OSS-Benchmark/ad4c0b92970a6f4801f09ff2e647389e8f5874fa/LICENSE) | HTTP 404 at this path; redistribution permission for questions/PDFs was not established. No upstream code or dataset copied here. A missing root file alone is not a claim that every individual PDF lacks a licence. |
| [FinanceBench dataset card](https://huggingface.co/datasets/PatronusAI/financebench/blob/e04404e3a97f69f79c14d42f24981a1c9c3bcd18/README.md) | Explicit `cc-by-nc-4.0`, 150 annotated examples. Not an unrestricted commercial redistribution grant. |
| [FinanceBench README](https://github.com/patronus-ai/financebench/blob/cc39aeb4afdf33909ee1412188bf89035950c2eb/README.md) / [root LICENSE request](https://raw.githubusercontent.com/patronus-ai/financebench/cc39aeb4afdf33909ee1412188bf89035950c2eb/LICENSE) | README describes the public sample; root LICENSE returned 404. Public company filings are not automatically public-domain. Dataset card does not establish rights to republish each underlying filing. No FinanceBench answers/PDFs committed. |

## Before a head-to-head model run

**Settled approach A:** use one identical shared agent/controller over both
systems' navigation tools to isolate retrieval-tool differences. The live A
adapter prerequisite is verification of pinned public local PageIndex tree/page
APIs; a guessed cloud method is not a local OSS contract.

A separately labelled future track B could compare native PageIndex `responses()`
against an anymd agent with the same model/prompt/budget. B compares complete
pipelines, including different controller behavior. The synthetic Responses
replay here is offline contract evidence for B only, not an A implementation or
retrieval-quality result. The controller choice for the main comparison is not
an outstanding decision.

Then freeze and record all of the following before making a live adapter opt-in:

1. A pinned installed PageIndex version and verified public local tree/page APIs;
   anymd binary containing the merged outline/node feature. No local Rust builds
   are required by this harness.
2. Identical PDF bytes, public questions, evidence pages and corpus membership.
   For upstream datasets, record per-document rights and whether redistribution
   is permitted. Download locally by a manifest rather than committing unverified
   FinanceBench/OSS Benchmark data. Preserve failures in a common denominator;
   label an upstream flash-compatible subset separately from broader coverage.
3. One exact model revision/provider for answering, identical system instructions,
   citation format, turn/context/output budgets, effort, seed where supported,
   temperature, tokenizer and grading policy. Keep answer annotations hidden.
   Choose and disclose PageIndex's separate indexing model and cached tree hashes.
4. Explicit inference opt-in and a lead-approved funding/credentials path. Model
   ID, rates with source/date/currency, maximum spend, index/query cost metadata,
   cache accounting and actual usage receipts are prerequisites. No provider
   credentials are included or read here, and no paid/model run was executed.
5. A saved per-question transcript, answer, citations, wall timings, token usage,
   index provenance, failures and judge inputs. If using a semantic model judge,
   keep it identical/blind and disclose its cost separately; otherwise publish
   exact-match limitations. Repeat runs and publish uncertainty, not a guaranteed
   win. A deterministic smoke result is neither real-world QA nor an extraction
   score.
