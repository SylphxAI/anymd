# Predictable defaults

Citra keeps the default path cheap and deterministic. `fast` is the default for
embedded text, metadata, structure, and bounded table work. `quality` explicitly
requests OCR, rendering, and richer crops. `research` is not hidden inside PDF
reading. Expensive work is requested, never silently triggered.
