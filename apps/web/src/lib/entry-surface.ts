// Only product-owned labels may leave the browser. Never send raw URL input.
const surfaces = [
  'pricing', 'pro', 'studio_cite_preview', 'cli_pdf', 'cli_video', 'npm_citra', 'npm_pdf_reader_mcp',
  'seo_pdf-and-office-to-markdown-mcp', 'seo_verifiable-video-and-cite-check-ai-agents',
  'seo_vs-docling', 'seo_vs-hub', 'seo_vs-kreuzberg', 'seo_vs-llamaparse',
  'seo_vs-markdownify-mcp', 'seo_vs-marker', 'seo_vs-markitdown',
  'seo_vs-mistral-ocr', 'seo_vs-pdftotext', 'seo_vs-pymupdf4llm', 'seo_vs-unstructured',
] as const;
type EntrySurface = typeof surfaces[number];

export function knownEntrySurface(value: unknown): EntrySurface | undefined {
  return surfaces.find(surface => surface === value);
}
