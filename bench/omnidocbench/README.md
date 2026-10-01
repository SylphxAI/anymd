# anymd on OmniDocBench v1.6

[OmniDocBench](https://github.com/opendatalab/OmniDocBench) (CVPR 2025) is the standard benchmark for turning
document pages into Markdown: 1,651 pages, 10 document types, scored on text, display formulas, tables, and
reading order. The published results of PaddleOCR-VL, MinerU2.5-Pro, OvisOCR2 and GLM-OCR are on it. This
directory runs anymd on it, so its numbers can sit next to theirs. The results are in
[docs/guide/benchmarks.md](../../docs/guide/benchmarks.md#omnidocbench-v16).

## What is measured

- **Variant: page images.** anymd gets each page as a PNG or JPEG; the workflow selects `ANYMD_OCR=vlm` or `tesseract`. The historical tesseract baseline uses English-only plain text, with no layout, table or formula model. The VLM route explicitly installs pinned weights, then uses PaddleOCR-VL-1.6 and PP-DocLayoutV3 on CPU, returning reading-order Markdown tables and LaTeX formulas. Each route goes through the same official evaluator.
- **Variant: source PDFs, not run.** The v1.6 release on Hugging Face ships page images and annotations only. There
  are no source PDFs to run anymd's native text-layer engine on, so this benchmark has no PDF number. (AgentDocBench
  in [`../README.md`](../README.md) is the benchmark that exercises the PDF engine.)
- **Adapter step.** anymd prints an image as a metadata table (format, size, EXIF) followed by a `## Text (OCR)`
  section. `harness.py` keeps only the OCR text; the metadata table is not page content. A page where anymd fails or
  finds no text gets an empty file and scores as such.

## Pinned versions

| What | Pin |
|---|---|
| Dataset | [`opendatalab/OmniDocBench`](https://huggingface.co/datasets/opendatalab/OmniDocBench) at Hugging Face commit `d386947f7fc3bafdcd756c8485845a2f43a19875` ("add v1.6") |
| Annotations | `OmniDocBench.json`, SHA-256 `a45cd84b04ad8b793e775089640e6b681209abea33ead54c1828ddca35fae496` |
| Page images | 1,651 files, each verified against [`images.sha256`](images.sha256) (the SHA-256 Hugging Face records) |
| Evaluator | `opendatalab/OmniDocBench` at commit `147cd5ac9472002f5751221d390bf00abdbc0d2f` (the v1.6 code release) |
| Evaluator config | its `configs/end2end.yaml` (end2end, quick_match; text, display formula CDM, table TEDS, reading order) |
| Evaluator runtime | Python 3.10, the evaluator's pinned `pyproject.toml` dependencies |

The evaluator's `main` branch and the Hugging Face dataset have since moved on to v1.7 labelling, which is why both
are pinned by commit. The annotation file is byte-identical at the v1.6 commit and at the current dataset head.

Changes to the evaluator config: the two data paths, and `match_workers`, `cdm_workers`, `teds_workers` lowered from
13 to 2 because the runner has 4 cores (the evaluator's README says to use a third to a half of the cores). No metric
is changed. The CDM formula metric renders LaTeX through `magick`; the runner has ImageMagick 6, so a `magick`
wrapper calls its `convert` (the evaluator's reference runtime is ImageMagick 7.1.1-47, TeX Live 2025). A CDM value
can differ slightly between renderers.

## Rerun

Everything runs on GitHub-hosted runners; nothing is run locally.

```bash
gh workflow run omnidocbench.yml --ref main -f ocr=vlm -f limit=0  # all 1,651 pages
gh workflow run omnidocbench.yml --ref main -f ocr=vlm -f limit=40   # smoke test: the first 40 pages by file name
```

The [OmniDocBench workflow](../../.github/workflows/omnidocbench.yml) builds anymd in release mode, converts the
pages in 32 shards (one model worker per runner), then runs the evaluator and uploads the `omnidocbench-results` artifact (the evaluator's result
JSON files, the config it ran with, and per-page anymd timings). The job summary shows the headline scores, computed from the metric files with the plain formula; the published figures use the evaluator's own `run_summary.json`, whose per-metric page counts differ slightly.
`summarize.py` computes Overall as the leaderboard does: ((1 - text edit distance) x 100 + table TEDS + formula CDM) / 3.

### Reuse completed predictions

To recover an evaluator failure without repeating inference, dispatch this workflow with `predictions_run` set to the completed source run id the full `predictions_sha` source commit, and the same `ocr` and `limit` values. For example:

```bash
gh workflow run omnidocbench.yml --ref <fixed-branch> -f ocr=vlm -f limit=0 -f predictions_run=<completed-run-id> -f predictions_sha=<full-source-sha>
```

This skips the build and prediction jobs, downloads only that run's prediction artifacts, and verifies the source workflow/SHA, all 32 successful shards, and the source log’s engine and limit before scoring. Prediction filenames and timing records must cover the selected ground truth exactly once. Provenance verification makes three one-shot API reads in CI; it never polls. The summary records the source run. Scores certify those source predictions, not inference on the recovery workflow's newer commit. Wait until every source prediction shard has completed and its artifacts are retained before dispatching recovery.

## Licence

The dataset is downloaded at run time and never redistributed; see its licence below.

- Evaluator code: Apache-2.0.
- Dataset: the OmniDocBench copyright statement says the PDFs are collected from public online channels and
  community contributions, and that "the dataset is for research purposes only and not for commercial use".
  This use is an evaluation: the workflow downloads the dataset at run time, scores anymd on it, and publishes the
  scores. Nothing from the dataset is committed here, redistributed, or used for training. Do not extend this
  directory to do any of those without asking the dataset owners (OpenDataLab@pjlab.org.cn).
