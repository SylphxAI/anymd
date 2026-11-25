type SourceUnit = { label: string; text: string };
export type QuoteMatch = { label: string; before: string; matched: string; after: string };

const clean = (text: string) => text.normalize('NFKC').replace(/\s+/g, ' ').trim();
export const normalizeQuote = (text: string) => clean(text).toLowerCase();

function excerpt(text: string, folded: string, start: number, length: number) {
  // The excerpt uses the same whitespace and Unicode normalization as the search,
  // but retains the document's case. Lowercasing can expand some letters (İ).
  let end = start + length;
  if (folded.length !== text.length) {
    let offset = 0;
    let original = 0;
    let first: number | undefined;
    for (const char of text) {
      offset += char.toLowerCase().length;
      first ??= offset > start ? original : undefined;
      original += char.length;
      if (offset >= end) { start = first ?? 0; end = original; break; }
    }
  }
  const matched = text.slice(start, end);
  const room = Math.max(0, 140 - matched.length);
  const left = Math.min(start, Math.floor(room / 2) + Math.max(0, Math.ceil(room / 2) - (text.length - end)));
  const right = Math.min(text.length - end, room - left);
  // Round outward at surrogate boundaries so clipping never breaks an emoji.
  const from = start - left - (isLowSurrogate(text, start - left) ? 1 : 0);
  const to = end + right + (isLowSurrogate(text, end + right) ? 1 : 0);
  return {
    before: (from > 0 ? '…' : '') + text.slice(from, start),
    matched,
    after: text.slice(end, to) + (to < text.length ? '…' : ''),
  };
}

function isLowSurrogate(text: string, index: number) {
  const code = text.charCodeAt(index);
  return code >= 0xdc00 && code <= 0xdfff;
}

export function findQuoteMatches(units: SourceUnit[], needle: string): QuoteMatch[] {
  const matches: QuoteMatch[] = [];
  if (needle.length < 3) return matches;
  for (const { label, text } of units) {
    const display = clean(text);
    const folded = display.toLowerCase();
    const start = folded.indexOf(needle);
    if (start < 0) continue;
    matches.push({ label, ...excerpt(display, folded, start, needle.length) });
    if (matches.length === 3) break;
  }
  return matches;
}
