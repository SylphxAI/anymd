/**
 * anymd SDK — programmatic document-to-Markdown API (Sylphx).
 *
 * Today this is a typed façade over the pure-Rust MCP server client.
 * Semantics match MCP tools: read_pdf, search_pdf, pdf_evidence.
 *
 * @example
 * ```ts
 * import { Anymd } from '@sylphx/anymd/sdk'
 * const anymd = Anymd.create()
 * const { payload, isError } = await anymd.read({ sources: [{ path: '/abs/doc.pdf' }] })
 * ```
 */
import {
  createPureRustClient,
  type PureRustCallResult,
  PureRustClient,
  type PureRustClientOptions,
  resolvePureRustServerBinary,
} from './pure-rust.js';

export type { PureRustCallResult, PureRustClientOptions };
export { createPureRustClient, PureRustClient, resolvePureRustServerBinary };

export type PdfSource = {
  path?: string;
  url?: string;
  pages?: number[] | string;
};

export type AnymdReadInput = {
  sources: PdfSource[];
  auto?: boolean;
  auto_detail?: 'fast' | 'balanced' | 'full';
  [key: string]: unknown;
};

export type AnymdSearchInput = {
  sources: PdfSource[];
  query?: string;
  queries?: string[];
  [key: string]: unknown;
};

export type AnymdEvidenceInput = {
  sources: PdfSource[];
  operation: string;
  [key: string]: unknown;
};

/** anymd — document instrument client */
export class Anymd {
  private readonly client: PureRustClient;

  constructor(options: PureRustClientOptions = {}) {
    this.client = createPureRustClient(options);
  }

  static create(options?: PureRustClientOptions): Anymd {
    return new Anymd(options);
  }

  /** Agent Document Twin extraction (MCP: read_pdf). */
  read(input: AnymdReadInput): Promise<PureRustCallResult> {
    return this.client.readPdf(input as Record<string, unknown>);
  }

  /** Cheap literal search with evidence (MCP: search_pdf). */
  search(input: AnymdSearchInput): Promise<PureRustCallResult> {
    return this.client.searchPdf(input as Record<string, unknown>);
  }

  /** Focused evidence ops: inspect/render/crop/ocr/... (MCP: pdf_evidence). */
  evidence(input: AnymdEvidenceInput): Promise<PureRustCallResult> {
    return this.client.pdfEvidence(input as Record<string, unknown>);
  }

  /** Escape hatch for raw tool names. */
  call(
    tool: 'read_pdf' | 'search_pdf' | 'pdf_evidence',
    args: Record<string, unknown>
  ): Promise<PureRustCallResult> {
    return this.client.callTool(tool, args);
  }
}

/** @deprecated Renamed to {@link Anymd}. Kept so `@sylphx/citra` era imports keep working. */
export const Citra = Anymd;
/** @deprecated Renamed to {@link Anymd}. */
export type Citra = Anymd;
/** @deprecated Renamed to {@link AnymdReadInput}. */
export type CitraReadInput = AnymdReadInput;
/** @deprecated Renamed to {@link AnymdSearchInput}. */
export type CitraSearchInput = AnymdSearchInput;
/** @deprecated Renamed to {@link AnymdEvidenceInput}. */
export type CitraEvidenceInput = AnymdEvidenceInput;

export default Anymd;
