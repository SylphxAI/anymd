# Agent-Native PDF Inspection

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: superseded by V3 smart tool surface

> V3 note: the public inspection path is now `pdf_evidence` operation
> `inspect`, and `read_pdf` performs automatic inspection by default when no
> explicit `include_*` options are supplied.

## Goal

Expose lightweight inspection in the V3 MCP surface before or during extraction.
The inspection path should answer: what kind of PDF is this, which pages should
be sampled, what risks are visible, which `read_pdf` options are most useful
next, and which follow-up evidence operations should be used next for search,
visual evidence, OCR, crop extraction, or provider-backed enrichment.

This improves time-to-value without adding bundled OCR models, Java, Python,
cloud services, or any large default dependency.

## Non-goals

- Do not bundle or automatically perform OCR in the default package.
- Do not claim high-accuracy parsing from a short inspection sample.
- Do not extract image bytes during inspection.
- Do not replace `read_pdf`; inspection is an additive planning step.

## Contract

Expose inspection as `pdf_evidence` with `operation: "inspect"`. `read_pdf`
uses the same inspection path automatically when no explicit `include_*` options
are supplied.

Input:

```json
{
  "operation": "inspect",
  "sources": [{ "path": "report.pdf", "pages": "1-5" }],
  "sample_pages": 5,
  "include_metadata": true
}
```

Output is a JSON text part:

```json
{
  "results": [
    {
      "source": "report.pdf",
      "success": true,
      "data": {
        "profile": "digital_text",
        "num_pages": 12,
        "sampled_pages": [1, 4, 7, 9, 12],
        "page_signals": [
          {
            "page": 1,
            "text_chars": 2400,
            "text_items": 82,
            "estimated_tokens": 600,
            "image_paint_operations": 2,
            "likely_scanned": false,
            "low_text_density": false
          }
        ],
        "document_signals": {
          "has_outline": true,
          "has_page_labels": false,
          "has_permissions": false,
          "has_mark_info": false,
          "has_form_fields": false,
          "has_attachments": false,
          "has_structure_tree": false
        },
        "recommendation": {
          "workflow": "agentic_rag",
          "needs_ocr": false,
          "reason": "Sampled pages expose selectable text; citation chunks, semantic hints, table extraction, and safety findings are the highest-value next read_pdf options.",
          "read_pdf_arguments": {
            "sources": [{ "path": "report.pdf" }],
            "include_metadata": true,
            "include_page_count": true,
            "include_page_geometry": true,
            "include_outline": true,
            "include_chunks": true,
            "include_semantic_hints": true,
            "include_safety_findings": true,
            "include_markdown": true,
            "include_tables": true
          },
          "next_tools": [
            {
              "tool": "read_pdf",
              "priority": 1,
              "ready": true,
              "purpose": "Build citation-ready agent context with document map, text-layer and metadata coverage, chunks, semantic hints, tables, layout diagnostics, safety findings, trust signal routing, and accessibility issue routing.",
              "when": "Use first when sampled pages already expose selectable text.",
              "arguments": {
                "sources": [{ "path": "report.pdf", "pages": "1-5" }],
                "include_metadata": true,
                "include_page_count": true,
                "include_page_geometry": true,
                "include_outline": true,
                "include_chunks": true,
                "include_semantic_hints": true,
                "include_safety_findings": true,
                "include_markdown": true,
                "include_tables": true
              }
            },
            {
              "tool": "search_pdf",
              "priority": 2,
              "ready": false,
              "purpose": "Find task-relevant source snippets with offsets, page references, and bbox evidence before heavier extraction.",
              "when": "Use before broad extraction when the task asks for specific facts, terms, or citations.",
              "argument_template": {
                "sources": [{ "path": "report.pdf", "pages": "1-5" }],
                "query": "<literal-query-from-user-task>",
                "include_ocr_text_layer": false,
                "max_matches_per_source": 10,
                "context_chars": 160
              },
              "required_inputs": ["literal search query"]
            },
            {
              "tool": "pdf_evidence",
              "priority": 3,
              "ready": false,
              "purpose": "Crop bbox-grounded regions as focused visual evidence after read_pdf exposes table, image, text-layer, or chunk boxes.",
              "when": "Use when read_pdf returns bbox evidence for a table, figure, chart, formula, annotation, or citation that needs visual proof.",
              "argument_template": {
                "operation": "extract_regions"
              },
              "required_inputs": ["page number", "PDF-coordinate bounding box"]
            },
            {
              "tool": "pdf_evidence",
              "priority": 4,
              "ready": false,
              "purpose": "Send focused crops to a configured local visual provider and normalize table, chart, formula, figure, or image-description evidence.",
              "when": "Use when a known region needs local visual table, chart, formula, figure, or image-description enrichment.",
              "argument_template": {
                "operation": "analyze_regions"
              },
              "required_inputs": ["page number", "PDF-coordinate bounding box"],
              "requires_provider": "analyze_regions"
            }
          ]
        },
        "provider_status": {
          "ocr_pages": {
            "readiness": "ready",
            "provider": "command",
            "command_configured": true,
            "health": "not_checked",
            "health_check": "not_checked",
            "preset": "tesseract"
          },
          "analyze_regions": {
            "readiness": "not_configured",
            "provider": "command",
            "command_configured": false,
            "health": "not_checked",
            "health_check": "not_checked",
            "warnings": [
              "Set MCP_PDF_REGION_ANALYSIS_COMMAND, MCP_PDF_REGION_ANALYSIS_HTTP_URL, or MCP_PDF_REGION_ANALYSIS_PRESET=ollama/openai-compatible/lmstudio/llamacpp to enable analyze_regions."
            ]
          }
        }
      }
    }
  ]
}
```

## Invariants

- Inspection returns per-source success or failure independently.
- Sampling is bounded and defaults to a small number of pages.
- `read_pdf_arguments` may recommend `include_ocr_text_layer` and
  `include_tables` together for scanned-page OCR table evidence, but must make
  OCR opt-in and dependent on a configured local provider with an available
  preset executable when a built-in OCR preset is selected.
- `next_tools` is ordered by `priority`; executable steps use `arguments`, while
  steps that need task input or region boxes use `argument_template` and
  `required_inputs`.
- `next_tools[].ready` means the step can be called immediately with the current
  arguments and provider readiness. OCR or visual-provider steps must become
  not ready when their provider is not configured, has invalid configuration, or
  the selected known OCR preset executable is unavailable.
- Provider readiness and health metadata must not expose local provider paths
  or arguments.
- The first JSON part must not include binary image data.
- Existing `read_pdf` callers remain unchanged.

## Validation

- Unit tests cover page sampling, profile classification, and ordered
  tool-routing recommendations.
- Handler tests cover successful digital and scanned/image-like PDFs.
- Integration tests confirm the MCP server lists `pdf_evidence`, supports
  `operation: "inspect"`, and keeps `read_pdf` automatic inspection enabled by
  default.
- Full validation: `bun run check`, `bun run build`, `bun test`, and
  `bun run docs:build`.
