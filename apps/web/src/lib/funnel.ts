import { currencyScale, price } from './catalog';
import type { Choice } from './consent';

// Only product-owned labels may leave the browser. Never send raw URL input.
const surfaces = [
  'pricing', 'pro', 'studio_cite_preview', 'cli_pdf', 'cli_video', 'npm_citra', 'npm_pdf_reader_mcp',
  'seo_pdf-and-office-to-markdown-mcp', 'seo_verifiable-video-and-cite-check-ai-agents',
  'seo_vs-docling', 'seo_vs-hub', 'seo_vs-kreuzberg', 'seo_vs-llamaparse',
  'seo_vs-markdownify-mcp', 'seo_vs-marker', 'seo_vs-markitdown',
  'seo_vs-mistral-ocr', 'seo_vs-pdftotext', 'seo_vs-pymupdf4llm', 'seo_vs-unstructured',
] as const;
type EntrySurface = typeof surfaces[number];

export function checkoutSurface(pathname: string, search: string): EntrySurface {
  const values = new URLSearchParams(search).getAll('entry_surface');
  return (values.length === 1 && surfaces.find(surface => surface === values[0])) || (pathname === '/pro' ? 'pro' : 'pricing');
}

export function explicitAnalytics(choice: Choice | null, gpc: boolean) {
  return !gpc && (choice === 'analytics' || choice === 'all');
}

export function checkoutParameters(currency: string, pathname: string, search: string) {
  const amounts = price.unit_amounts;
  if (!Object.hasOwn(amounts, currency)) return;
  const value = Number(amounts[currency as keyof typeof amounts]) / currencyScale(currency);
  if (!Number.isFinite(value) || value <= 0) return;
  return {value, currency, plan_sku:price.key, entry_surface:checkoutSurface(pathname, search), items:[{item_id:price.key, item_name:'Anymd Pro', quantity:1, price:value}]};
}
