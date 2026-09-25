# Visual Region Analysis Provider

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: shipped

> V3 note: visual-region analysis is now exposed through `pdf_evidence`
> operation `analyze_regions`.

`analyze_regions` turns focused PDF crops into normalized local-provider
enrichment. It is the provider boundary for visual table recognition,
chart-to-data extraction, formula recognition, figure descriptions, and image
captions.

`read_pdf` can reuse the same provider through `include_visual_enrichments`.
It sends direct table/image element crops when available and can also derive
bounded visual regions from semantic captions such as `Formula`, `Chart`,
`Figure`, `Image`, or `Diagram`. Caption-derived enrichments keep a stable
synthetic target ID plus `source_caption_element_id`, `source_caption_text`,
and routing signals so agents can trace the crop back to source text evidence.

The default package does not bundle a vision model. A request cannot select an
executable or endpoint. Operators enable providers with environment variables:

- `MCP_PDF_REGION_ANALYSIS_COMMAND`
- `MCP_PDF_REGION_ANALYSIS_ARGS_JSON`
- `MCP_PDF_REGION_ANALYSIS_HTTP_URL`
- `MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON`
- `MCP_PDF_REGION_ANALYSIS_PRESET=ollama`
- `MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL`
- `MCP_PDF_REGION_ANALYSIS_OLLAMA_URL`
- `MCP_PDF_REGION_ANALYSIS_PRESET=openai-compatible`
- `MCP_PDF_REGION_ANALYSIS_OPENAI_MODEL`
- `MCP_PDF_REGION_ANALYSIS_OPENAI_URL`
- `MCP_PDF_REGION_ANALYSIS_OPENAI_API_KEY`
- `MCP_PDF_REGION_ANALYSIS_PRESET=lmstudio`
- `MCP_PDF_REGION_ANALYSIS_LMSTUDIO_MODEL`
- `MCP_PDF_REGION_ANALYSIS_LMSTUDIO_URL`
- `MCP_PDF_REGION_ANALYSIS_PRESET=llamacpp`
- `MCP_PDF_REGION_ANALYSIS_LLAMACPP_MODEL`
- `MCP_PDF_REGION_ANALYSIS_LLAMACPP_URL`

Command providers take precedence when both command and HTTP providers are
configured. The args template must include `{input}` and may also use `{page}`, `{source}`,
`{region_id}`, `{evidence_id}`, `{left}`, `{bottom}`, `{right}`, `{top}`,
`{language}`, and `{languages}`.

HTTP providers receive a JSON POST payload with:

```json
{
  "image_base64": "...",
  "mime_type": "image/png",
  "format": "png",
  "page": 2,
  "region_id": "chart-1",
  "evidence_id": "page-2-chart-1-crop-scale-2",
  "source": "documents/report.pdf",
  "source_bounding_box": { "left": 72, "bottom": 240, "right": 540, "top": 520 },
  "crop_pixels": { "left": 144, "top": 544, "width": 936, "height": 560 },
  "scale": 2,
  "languages": ["eng"]
}
```

The Ollama preset is a local HTTP preset over Ollama `/api/generate`. It sends
the cropped PNG as a base64 entry in `images`, sets `stream: false`, requests
`format: "json"`, and normalizes the JSON object returned in Ollama's
`response` string into the same evidence contract. Operators must provide the
local model name with `MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL`; the endpoint
defaults to `http://127.0.0.1:11434/api/generate`. The package does not bundle
Ollama or a model.

The OpenAI-compatible preset targets local or private chat-completions vision
servers. Operators must provide `MCP_PDF_REGION_ANALYSIS_OPENAI_URL` and
`MCP_PDF_REGION_ANALYSIS_OPENAI_MODEL`; there is no remote default endpoint.
The request body uses a `messages` array with a JSON-only text prompt and an
`image_url` data URL for the cropped PNG. If
`MCP_PDF_REGION_ANALYSIS_OPENAI_API_KEY` is set, it is sent as the bearer token
and replaces any `authorization` entry from
`MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON`. The response parser normalizes
`choices[0].message.content` into the same evidence contract.

The LM Studio and llama.cpp presets are local OpenAI-compatible chat-completions
presets. They use the same request and response contract as
`openai-compatible`, but default to common localhost endpoints:
`http://127.0.0.1:1234/v1/chat/completions` for LM Studio and
`http://127.0.0.1:8080/v1/chat/completions` for llama.cpp. Operators must set
the matching model env var for the loaded local vision model or alias, and may
override the endpoint with the matching URL env var.

## Request

`analyze_regions` accepts the same region source shape as `extract_regions`:

```json
{
  "sources": [{
    "path": "documents/report.pdf",
    "regions": [{
      "id": "chart-1",
      "page": 2,
      "bounding_box": { "left": 72, "bottom": 240, "right": 540, "top": 520 },
      "padding": 8
    }]
  }],
  "scale": 2,
  "max_regions": 10,
  "languages": ["eng"]
}
```

The server renders each requested page within `max_pixels_per_page`, crops the
requested PDF-coordinate bbox, then invokes the configured provider. Command
providers receive a temporary PNG path that is deleted after the attempt. HTTP
providers receive the crop bytes in the env-configured JSON request body.

## Provider Output

Command stdout or HTTP response body may be plain text or JSON. JSON output can
include:

```json
{
  "kind": "table",
  "description": "Quarterly revenue table",
  "text": "Q1 revenue...",
  "markdown": "| Quarter | Revenue |",
  "confidence": 0.91,
  "table": {
    "rows": [["Quarter", "Revenue"], ["Q1", "$1.2M"]],
    "row_count": 2,
    "column_count": 2,
    "cells": [{
      "text": "Quarter",
      "row_index": 0,
      "column_index": 0,
      "row_span": 1,
      "column_span": 1,
      "bounding_box": { "left": 72, "bottom": 492, "right": 168, "top": 520 },
      "confidence": 0.94
    }],
    "markdown": "| Quarter | Revenue |",
    "csv": "Quarter,Revenue\nQ1,$1.2M",
    "confidence": 0.9
  },
  "formula": {
    "latex": "E = mc^2",
    "mathml": "<math><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></math>",
    "asciimath": "E = m c^2",
    "text": "E equals m c squared",
    "confidence": 0.82
  },
  "chart": {
    "title": "Revenue by quarter",
    "summary": "Revenue rises across the period.",
    "data_points": [{ "label": "Q1", "value": 1.2 }],
    "x_axis": { "label": "Quarter" },
    "y_axis": { "label": "Revenue", "unit": "USD millions", "min": 0 },
    "series": [{
      "name": "Revenue",
      "data_points": [{ "label": "Q1", "value": 1.2 }],
      "confidence": 0.78
    }],
    "confidence": 0.78
  },
  "warnings": ["Low contrast axis labels"]
}
```

Supported `kind` values are `text`, `table`, `figure`, `chart`, `formula`,
`image`, `diagram`, and `unknown`. Unknown provider values are normalized to
`unknown` with a warning. Confidence values are clamped to `0..1`.

Table cells may include zero-based `row_index`, `column_index`, optional
`row_span`, `column_span`, confidence, and PDF-coordinate bounding boxes. Chart
outputs may include top-level data points plus axes and named series. Formula
outputs may include LaTeX, MathML, AsciiMath, and plain text.

## Response

The first content part is JSON:

```json
{
  "profile": "region_analysis",
  "analysis_options": {
    "scale": 2,
    "max_regions": 20,
    "max_pixels_per_page": 16000000,
    "timeout_ms": 60000,
    "max_output_chars": 200000
  },
  "results": [{
    "source": "documents/report.pdf",
    "success": true,
    "num_pages": 8,
    "region_analyses": [{
      "region_id": "chart-1",
      "page": 2,
      "kind": "chart",
      "description": "Revenue chart",
      "provider": "command",
      "source_crop_evidence_id": "page-2-chart-1-crop-scale-2",
      "provenance": {
        "engine": "external-command",
        "source": "region-analysis-provider"
      }
    }]
  }]
}
```

Every analyzed region includes the crop evidence ID, source bounding box, crop
pixel bounds, scale, provider, and provenance. The JSON response does not
duplicate cropped image base64.

## Provider Certification

`bun run benchmark:providers` is the installed-provider certification path for
the same contract. When a visual-region provider is configured, the benchmark
creates a runtime PDF fixture with separate table, formula, chart, figure, and
image-description regions, crops each region through the PDF renderer, invokes
the configured provider, and emits a `visual-full-fidelity` certification
summary.

The profile passes only when the provider:

- analyzes every certification region;
- preserves crop evidence provenance for every region;
- returns a structured table with cell bounding boxes;
- returns machine-readable formula evidence in at least two formats; and
- returns chart axes plus series or data points;
- returns figure description evidence; and
- returns image-description evidence.

This profile proves that a configured provider satisfies the Agent Document
Twin visual-evidence contract for the fixture set. It is still not a universal
model-accuracy claim; broader domain accuracy requires larger public fixture
runs that name the configured engine and fixture corpus.

The repository includes `scripts/reference-region-analysis-provider.mjs` as a
deterministic command provider for this certification fixture set. Release
workflows use it to prove the visual provider contract and crop-provenance
normalization path. It is intentionally fixture-scoped and does not bundle or
claim a general vision model.

The Ollama and OpenAI-compatible presets are covered by unit tests for request
shape, prompt routing, credential/header handling, and response normalization.
Model-specific quality still requires provider benchmark runs that name the
configured model and fixture corpus.

## Non-Goals

- No bundled model weights.
- No request-controlled command execution or request-controlled provider URL.
- No claim of PDF/UA, scientific formula, or chart extraction certification.
- No benchmark claim for a provider unless the configured engine and fixture set
  are reported separately.
