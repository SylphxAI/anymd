// Only product-owned labels may reach analytics or checkout metadata.
const proCtaSurfaces = [
  'home_hero', 'home_pro_panel', 'article_bridge', 'studio_cite_preview',
  'task_build_video_timeline', 'task_check_pdf_citation', 'task_extract_video_frame',
  'seo_pdf-and-office-to-markdown-mcp', 'seo_verifiable-video-and-cite-check-ai-agents',
  'seo_vs-docling', 'seo_vs-hub', 'seo_vs-kreuzberg', 'seo_vs-llamaparse',
  'seo_vs-markdownify-mcp', 'seo_vs-marker', 'seo_vs-markitdown',
  'seo_vs-mistral-ocr', 'seo_vs-pdftotext', 'seo_vs-pymupdf4llm', 'seo_vs-unstructured',
] as const;
const surfaces = ['pricing', 'pro', 'cli_pdf', 'cli_video', 'npm_citra', 'npm_pdf_reader_mcp', ...proCtaSurfaces] as const;
export type EntrySurface = typeof surfaces[number];
export type ProCtaSurface = typeof proCtaSurfaces[number];

export function knownEntrySurface(value: unknown): EntrySurface | undefined {
  if (typeof value !== 'string' || value.length < 1 || value.length > 100 || /[^a-z0-9_-]/.test(value)) return;
  return surfaces.find(surface => surface === value);
}

export function knownProCtaSurface(value: unknown): ProCtaSurface | undefined {
  const surface = knownEntrySurface(value);
  return proCtaSurfaces.find(allowed => allowed === surface);
}
