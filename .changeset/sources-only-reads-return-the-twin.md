---
"@sylphx/citra": patch
---

A sources-only `read_pdf` returns the document twin again.

Every MCP caller was silently losing the headline path. The server materializes each source as `{"path": ..., "pages": null}`, and the auto resolver counted that `pages` key as an explicit option — so `auto` resolved to `false` on 100% of MCP reads and a sources-only call returned a four-key shell (`engine`, `info`, `num_pages`, `route`) instead of the Agent Document Twin.

A `pages: null` value means "not specified" and no longer disables auto. A sources-only call now returns `markdown`, `chunks`, `document_map`, `elements`, `page_geometry`, `layout_diagnostics`, `safety_findings`, `trust_report` and `accessibility_report`; `profile: fast` returns the lean twin; explicit `include_*` options still take full manual control.
