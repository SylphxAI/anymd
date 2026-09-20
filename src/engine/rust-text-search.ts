import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { PdfSearchMatch, SearchPdfOptions } from '../types/pdf.js';
import { cargoBinaryCandidates } from '../utils/cargoTargetDir.js';

type RustTextSearchMatchWire = {
  id: string;
  page: number;
  text: string;
  snippet: string;
  match_start: number;
  match_end: number;
  route: string;
};

type RustTextSearchResultWire = {
  num_pages: number;
  searched_pages: number[];
  total_matches: number;
  matches: RustTextSearchMatchWire[];
  route: string;
  truncated: boolean;
};

export type RustTextSearchResult = {
  numPages: number;
  searchedPages: number[];
  totalMatches: number;
  matches: Array<{
    id: string;
    page: number;
    text: string;
    snippet: string;
    matchStart: number;
    matchEnd: number;
    route: string;
  }>;
  route: string;
  truncated: boolean;
};

type RustTextSearchEnvelope =
  | { status: 'ok'; search: RustTextSearchResultWire }
  | { status: 'error'; code: string; message: string };

const mapWireResult = (search: RustTextSearchResultWire): RustTextSearchResult => ({
  numPages: search.num_pages,
  searchedPages: search.searched_pages,
  totalMatches: search.total_matches,
  route: search.route,
  truncated: search.truncated,
  matches: search.matches.map((match) => ({
    id: match.id,
    page: match.page,
    text: match.text,
    snippet: match.snippet,
    matchStart: match.match_start,
    matchEnd: match.match_end,
    route: match.route,
  })),
});

const here = path.dirname(fileURLToPath(import.meta.url));

export function resolveRustCliBinary(): string {
  const env = process.env['PDF_READER_CLI'];
  if (env && existsSync(env)) {
    return env;
  }

  for (const candidate of cargoBinaryCandidates(path.join(here, '../..'), 'pdf-reader-cli')) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  return 'pdf-reader-cli';
}

export function isRustCliAvailable(): boolean {
  return resolveRustCliBinary() !== 'pdf-reader-cli';
}

export function shouldUseRustTextSearchEngine(preferSpeed = false): boolean {
  if (process.env['PDF_READER_USE_RUST_TEXT_SEARCH'] === '0') {
    return false;
  }
  if (process.env['PDF_READER_USE_RUST_TEXT_SEARCH'] === '1') {
    return isRustCliAvailable();
  }
  if (preferSpeed) {
    return isRustCliAvailable();
  }
  return false;
}

export function searchPdfTextViaRustEngine(
  filePath: string,
  options: SearchPdfOptions
): { ok: true; result: RustTextSearchResult } | { ok: false; code: string; message: string } {
  const binary = resolveRustCliBinary();
  const payload = JSON.stringify({
    tool: 'pdf_text_search',
    input: {
      path: filePath,
      query: options.query,
      case_sensitive: options.case_sensitive,
      whole_word: options.whole_word,
      max_pages: options.max_pages,
      max_matches: options.max_matches_per_source,
      context_chars: options.context_chars,
    },
  });

  const response = spawnSync(binary, [], {
    input: payload,
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
  });

  if (response.error) {
    return {
      ok: false,
      code: 'ENGINE_UNAVAILABLE',
      message: response.error.message,
    };
  }

  if (response.status !== 0) {
    return {
      ok: false,
      code: 'ENGINE_FAILED',
      message: response.stderr || `Rust text search engine exited with status ${response.status}`,
    };
  }

  const envelope = JSON.parse(response.stdout) as RustTextSearchEnvelope;
  if (envelope.status !== 'ok') {
    return {
      ok: false,
      code: envelope.code,
      message: envelope.message,
    };
  }

  return { ok: true, result: mapWireResult(envelope.search) };
}

export const mapRustMatchesToPdfSearchMatches = (
  matches: RustTextSearchResult['matches']
): PdfSearchMatch[] =>
  matches.map((match) => ({
    id: match.id,
    page: match.page,
    text: match.text,
    snippet: match.snippet,
    match_start: match.matchStart,
    match_end: match.matchEnd,
    provenance: {
      engine: 'rust-text-index',
      source: 'text-content',
    },
  }));
