# Product demos

These are the shareable agent workflows for anymd.

| Demo | File | Story |
| --- | --- | --- |
| Read | [`../read-basic.json`](../read-basic.json) | One call → Markdown with page markers |
| Pages and budget | [`../read-pages.json`](../read-pages.json) | Chosen pages, a token budget, and a cursor |
| Search then verify | [`../search-then-verify.json`](../search-then-verify.json) | Find evidence, then render the page |
| OCR scan | [`../ocr-scanned.json`](../ocr-scanned.json) | Scanned page path |
| Visual crop | [`../evidence-crop.json`](../evidence-crop.json) | Citation crop |
| Structure | [`../inspect-structure.json`](../inspect-structure.json) | Structured JSON with trust reports |

## One-liner pitch

Plain-text tools make agents guess. anymd gives them Markdown with page
citations, and evidence when they need to check.

## Install

```bash
npm install -g @sylphx/anymd
claude mcp add anymd -- npx -y @sylphx/anymd
```
