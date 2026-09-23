# Vision — Citra

Citra is the local-first PDF evidence tool for agents.

- **Identity:** package `@sylphx/citra`, bin `citra`, MCP `io.github.SylphxAI/citra`, site <https://sylphxai.github.io/citra/>.
- **User:** an agent or developer who must answer from a PDF that cannot be uploaded.
- **Job:** turn one local PDF into structured text, tables, OCR, visual crops and page-level citations a human can check.
- **Promise:** one call returns locators, provenance, confidence, warnings and gaps; a claim without evidence is reported as a gap, not invented.
- **Defaults:** `fast` reads the embedded text layer and structure; `quality` explicitly requests OCR, rendering and richer crops; expensive work is never silently triggered.
- **Boundaries:** Citra owns PDF reading, comparison and evidence. It does not own cloud OCR, generative summarisation as authority, filesystem mutation, or deliberation.
