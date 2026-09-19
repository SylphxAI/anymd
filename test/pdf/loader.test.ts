import dns from 'node:dns';
import fs, * as realFsPromises from 'node:fs/promises';
import * as pdfjsLib from 'pdfjs-dist/legacy/build/pdf.mjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { __setFetchUrlHopForTests, loadPdfDocument } from '../../src/pdf/loader.js';
import { __resetSecurityConfigForTests } from '../../src/utils/config.js';
import { ErrorCode, PdfError } from '../../src/utils/errors.js';
import * as pathUtils from '../../src/utils/pathUtils.js';

vi.mock('node:fs/promises', () => ({
  ...realFsPromises,
  default: {
    ...realFsPromises,
    readFile: vi.fn(),
    stat: vi.fn(),
  },
  readFile: vi.fn(),
  stat: vi.fn(),
}));

vi.mock('pdfjs-dist/legacy/build/pdf.mjs', () => ({
  getDocument: vi.fn(),
}));

vi.mock('../../src/utils/pathUtils.js', () => ({
  resolvePath: vi.fn(),
}));

const buildStats = (size: number) =>
  ({
    size,
    isFile: () => true,
    isDirectory: () => false,
  }) as unknown as Awaited<ReturnType<typeof fs.stat>>;

const buildResponse = (
  body: Uint8Array,
  init: { status?: number; headers?: Record<string, string> } = {}
): Response => {
  const headers = new Headers(init.headers);
  if (!headers.has('content-length')) headers.set('content-length', String(body.byteLength));
  const response = new Response(body, { status: init.status ?? 200, headers });
  // jsdom's Response sometimes lacks redirect handling under bun; rely on
  // the global Response which `fetch` returns in Node 22.
  return response;
};

let originalFetch: typeof globalThis.fetch;

/**
 * The URL loader connects through a DNS-pinned agent (GHSA-5r2f-7788-qp8v), so
 * a `globalThis.fetch` stub no longer observes it. These tests stub the hop
 * fetch itself and can assert that it was called with the validated URL.
 */
const stubUrlHopFetch = (
  impl: (url: string, init: RequestInit) => Promise<Response>
): ReturnType<typeof vi.fn> => {
  const spy = vi.fn(impl);
  __setFetchUrlHopForTests(spy as unknown as Parameters<typeof __setFetchUrlHopForTests>[0]);
  return spy;
};

describe('loader', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    originalFetch = globalThis.fetch;
    __resetSecurityConfigForTests();
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    __setFetchUrlHopForTests(null);
  });

  describe('loadPdfDocument', () => {
    it('should load PDF from local file path', async () => {
      const mockBuffer = Buffer.from('fake pdf content');
      const mockDocument = { numPages: 5 };

      pathUtils.resolvePath.mockReturnValue('/safe/path/test.pdf');
      fs.stat.mockResolvedValue(buildStats(mockBuffer.length));
      fs.readFile.mockResolvedValue(mockBuffer);
      pdfjsLib.getDocument.mockReturnValue({
        promise: Promise.resolve(mockDocument as unknown as pdfjsLib.PDFDocumentProxy),
      } as pdfjsLib.PDFDocumentLoadingTask);

      const result = await loadPdfDocument({ path: 'test.pdf' }, 'test.pdf');

      expect(result).toBe(mockDocument);
      expect(pathUtils.resolvePath).toHaveBeenCalledWith('test.pdf');
      expect(fs.stat).toHaveBeenCalledWith('/safe/path/test.pdf');
      expect(fs.readFile).toHaveBeenCalledWith('/safe/path/test.pdf');
    });

    it('should reject oversized files via fs.stat before buffering (SSS-08)', async () => {
      pathUtils.resolvePath.mockReturnValue('/safe/path/huge.pdf');
      // 200MB > 100MB cap
      fs.stat.mockResolvedValue(buildStats(200 * 1024 * 1024));

      await expect(loadPdfDocument({ path: 'huge.pdf' }, 'huge.pdf')).rejects.toThrow(
        /exceeds maximum size/i
      );
      // Crucially, fs.readFile must NOT be called — the whole point of the
      // pre-check is to avoid buffering oversized data.
      expect(fs.readFile).not.toHaveBeenCalled();
    });

    it('should load PDF from URL via fetch (not pdfjs URL loader)', async () => {
      const body = new TextEncoder().encode('mock pdf body');
      const mockDocument = { numPages: 3 };
      const fetchMock = stubUrlHopFetch(async () => buildResponse(body));

      pdfjsLib.getDocument.mockReturnValue({
        promise: Promise.resolve(mockDocument as unknown as pdfjsLib.PDFDocumentProxy),
      } as pdfjsLib.PDFDocumentLoadingTask);

      const result = await loadPdfDocument(
        { url: 'https://example.com/test.pdf' },
        'https://example.com/test.pdf'
      );

      expect(result).toBe(mockDocument);
      expect(fetchMock).toHaveBeenCalledWith(
        'https://example.com/test.pdf',
        expect.objectContaining({ signal: expect.anything() })
      );
      // pdfjs now receives the body as `data`, not a `url`, so we control
      // size limits and SSRF policy ourselves.
      const opts = pdfjsLib.getDocument.mock.calls[0]?.[0] as { data: Uint8Array; url?: string };
      expect(opts.url).toBeUndefined();
      expect(opts.data).toBeInstanceOf(Uint8Array);
    });

    it('should reject URLs that resolve to private IPs (SSS-07)', async () => {
      // 169.254.169.254 is the AWS/GCP metadata endpoint; literal-IP hostname
      // is checked without DNS so the test stays hermetic.
      const fetchMock = stubUrlHopFetch(async () => buildResponse(new Uint8Array(0)));

      await expect(
        loadPdfDocument(
          { url: 'http://169.254.169.254/latest/meta-data/' },
          'http://169.254.169.254/'
        )
      ).rejects.toThrow(/non-public address|SSRF/);
      expect(fetchMock).not.toHaveBeenCalled();
    });

    // Regression tests for https://github.com/SylphxAI/pdf-reader-mcp/issues/368
    // (GHSA-34gp-w56h-r2mv): the url branch must not bypass path confinement via
    // SSRF or file:// local-file reads.
    it('should reject loopback URLs before any network fetch (issue #368)', async () => {
      const fetchMock = stubUrlHopFetch(async () => buildResponse(new Uint8Array(0)));

      await expect(
        loadPdfDocument(
          { url: 'http://127.0.0.1:8080/internal.pdf' },
          'http://127.0.0.1:8080/internal.pdf'
        )
      ).rejects.toThrow(/non-public address|SSRF/);
      expect(fetchMock).not.toHaveBeenCalled();
    });

    it('should reject file:// URLs instead of reading arbitrary local files (issue #368)', async () => {
      const fetchMock = stubUrlHopFetch(async () => buildResponse(new Uint8Array(0)));

      await expect(
        loadPdfDocument(
          { url: 'file:///tmp/word_outside/anywhere.pdf' },
          'file:///tmp/word_outside/anywhere.pdf'
        )
      ).rejects.toThrow(/Access denied|rejected/);
      expect(fetchMock).not.toHaveBeenCalled();
      expect(fs.readFile).not.toHaveBeenCalled();
      expect(pathUtils.resolvePath).not.toHaveBeenCalled();
    });

    it('should re-validate redirect targets and reject private-IP hops (issue #368)', async () => {
      const fetchMock = stubUrlHopFetch(
        async () =>
          new Response(null, {
            status: 302,
            headers: { location: 'http://169.254.169.254/latest/meta-data/' },
          })
      );

      await expect(
        loadPdfDocument(
          { url: 'https://example.com/redirect.pdf' },
          'https://example.com/redirect.pdf'
        )
      ).rejects.toThrow(/non-public address|SSRF/);
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    it('should reject URL responses whose Content-Length exceeds the cap (SSS-08)', async () => {
      stubUrlHopFetch(
        async () =>
          new Response(new Uint8Array(0), {
            status: 200,
            headers: { 'content-length': String(200 * 1024 * 1024) },
          })
      );

      await expect(
        loadPdfDocument({ url: 'https://example.com/huge.pdf' }, 'https://example.com/huge.pdf')
      ).rejects.toThrow(/exceeds maximum size/i);
    });

    // The DNS-rebinding regression (GHSA-5r2f-7788-qp8v) needs a real socket to
    // be meaningful, so it lives in test/pdf/rebind.test.ts.
    it('refuses a hop whose answer is non-public, before opening a socket', async () => {
      const resolved = vi
        .spyOn(dns.promises, 'lookup')
        .mockResolvedValue([{ address: '169.254.169.254', family: 4 }] as Awaited<
          ReturnType<typeof dns.promises.lookup>
        >);

      try {
        await expect(
          loadPdfDocument(
            { url: 'http://metadata.example/latest/meta-data/' },
            'http://metadata.example/'
          )
        ).rejects.toThrow(/non-public address|SSRF/);
      } finally {
        resolved.mockRestore();
      }
    });

    // Regression test for https://github.com/SylphxAI/pdf-reader-mcp/issues/271
    // pdfjs-dist requires absolute filesystem URLs for CMaps, standard fonts,
    // WASM decoders (OpenJPEG/QCMS), and ICC profiles. Missing any of them
    // breaks decoding — most visibly, a missing `wasmUrl` produces the
    // "Cannot find package 'nullopenjpeg_nowasm_fallback.js'" error because
    // pdfjs concatenates `null` with the fallback filename.
    it('should provide all pdfjs-dist resource URLs with trailing slashes to fix image decoding (issue #271)', async () => {
      const mockBuffer = Buffer.from('fake pdf content');
      const mockDocument = { numPages: 1 };

      pdfjsLib.getDocument.mockClear();
      pathUtils.resolvePath.mockReturnValue('/safe/path/test.pdf');
      fs.stat.mockResolvedValue(buildStats(mockBuffer.length));
      fs.readFile.mockResolvedValue(mockBuffer);
      pdfjsLib.getDocument.mockReturnValue({
        promise: Promise.resolve(mockDocument as unknown as pdfjsLib.PDFDocumentProxy),
      } as pdfjsLib.PDFDocumentLoadingTask);

      await loadPdfDocument({ path: 'test.pdf' }, 'test.pdf');

      expect(pdfjsLib.getDocument).toHaveBeenCalledTimes(1);
      const options = pdfjsLib.getDocument.mock.calls[0]?.[0] as {
        cMapUrl: string;
        cMapPacked: boolean;
        standardFontDataUrl: string;
        wasmUrl: string;
        iccUrl: string;
      };

      // All four URLs must be defined strings — an undefined `wasmUrl` is
      // what causes the `nullopenjpeg_nowasm_fallback.js` failure.
      expect(typeof options.cMapUrl).toBe('string');
      expect(typeof options.standardFontDataUrl).toBe('string');
      expect(typeof options.wasmUrl).toBe('string');
      expect(typeof options.iccUrl).toBe('string');

      // All URLs must point inside the installed pdfjs-dist package so they
      // work regardless of the consumer's cwd (e.g. when run via npx).
      expect(options.cMapUrl).toContain('pdfjs-dist');
      expect(options.standardFontDataUrl).toContain('pdfjs-dist');
      expect(options.wasmUrl).toContain('pdfjs-dist');
      expect(options.iccUrl).toContain('pdfjs-dist');

      // pdfjs-dist concatenates filenames directly onto these URLs, so each
      // one MUST end with a trailing slash. Without it, `${wasmUrl}openjpeg.wasm`
      // produces a malformed path and image decoding silently fails.
      expect(options.cMapUrl.endsWith('/')).toBe(true);
      expect(options.standardFontDataUrl.endsWith('/')).toBe(true);
      expect(options.wasmUrl.endsWith('/')).toBe(true);
      expect(options.iccUrl.endsWith('/')).toBe(true);

      // cMapPacked stays true — the bundled CMaps are binary-packed.
      expect(options.cMapPacked).toBe(true);
    });

    it('should throw PdfError when neither path nor url provided', async () => {
      await expect(loadPdfDocument({}, 'unknown')).rejects.toThrow(PdfError);
      await expect(loadPdfDocument({}, 'unknown')).rejects.toThrow(
        "Source unknown missing 'path' or 'url'."
      );
    });

    it('should handle file not found error (ENOENT)', async () => {
      const enoentError = Object.assign(new Error('File not found'), { code: 'ENOENT' });

      pathUtils.resolvePath.mockReturnValue('/safe/path/missing.pdf');
      fs.stat.mockRejectedValue(enoentError);

      await expect(loadPdfDocument({ path: 'missing.pdf' }, 'missing.pdf')).rejects.toThrow(
        PdfError
      );
      await expect(loadPdfDocument({ path: 'missing.pdf' }, 'missing.pdf')).rejects.toThrow(
        "File not found at 'missing.pdf'."
      );
    });

    it('should handle generic file stat errors with a sanitized message (SSS-02)', async () => {
      pathUtils.resolvePath.mockReturnValue('/safe/path/error.pdf');
      // EACCES (permission denied) should not leak the underlying message
      // verbatim — the LLM-facing string must not contain "Permission denied".
      fs.stat.mockRejectedValue(Object.assign(new Error('Permission denied'), { code: 'EACCES' }));

      await expect(loadPdfDocument({ path: 'error.pdf' }, 'error.pdf')).rejects.toThrow(PdfError);
      await expect(loadPdfDocument({ path: 'error.pdf' }, 'error.pdf')).rejects.toThrow(
        /Failed to access file/
      );
      await expect(loadPdfDocument({ path: 'error.pdf' }, 'error.pdf')).rejects.not.toThrow(
        /Permission denied/
      );
    });

    it('should reject non-regular files such as directories', async () => {
      pathUtils.resolvePath.mockReturnValue('/safe/path/dir');
      fs.stat.mockResolvedValue({
        size: 4096,
        isFile: () => false,
        isDirectory: () => true,
      } as unknown as Awaited<ReturnType<typeof fs.stat>>);

      await expect(loadPdfDocument({ path: 'dir' }, 'dir')).rejects.toThrow(/not a regular file/);
    });

    it('should handle PDF.js loading errors without leaking the raw message (SSS-02)', async () => {
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const mockBuffer = Buffer.from('fake pdf');

      pathUtils.resolvePath.mockReturnValue('/safe/path/bad.pdf');
      fs.stat.mockResolvedValue(buildStats(mockBuffer.length));
      fs.readFile.mockResolvedValue(mockBuffer);
      pdfjsLib.getDocument.mockImplementation(
        () =>
          ({
            promise: Promise.reject(new Error('Invalid PDF /private/internal/path.bin')),
          }) as pdfjsLib.PDFDocumentLoadingTask
      );

      try {
        await expect(loadPdfDocument({ path: 'bad.pdf' }, 'bad.pdf')).rejects.toThrow(PdfError);
        await expect(loadPdfDocument({ path: 'bad.pdf' }, 'bad.pdf')).rejects.toThrow(
          'Failed to load PDF document from bad.pdf.'
        );
        // The internal path must NOT make it into the surfaced error.
        await expect(loadPdfDocument({ path: 'bad.pdf' }, 'bad.pdf')).rejects.not.toThrow(
          /private\/internal/
        );

        // Logger still records the raw details for operators.
        expect(consoleErrorSpy).toHaveBeenCalledWith(
          expect.stringContaining('PDF.js loading error')
        );
      } finally {
        pdfjsLib.getDocument.mockReset();
        consoleErrorSpy.mockRestore();
      }
    });

    it('should handle non-Error PDF.js loading exceptions', async () => {
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      const body = new TextEncoder().encode('pdf body');
      stubUrlHopFetch(async () => buildResponse(body));

      // Build the rejected promise lazily inside mockImplementation so each
      // call gets a fresh rejection without leaving an unhandled one parked
      // at test-setup time.
      pdfjsLib.getDocument.mockImplementation(
        () => ({ promise: Promise.reject('Unknown error') }) as pdfjsLib.PDFDocumentLoadingTask
      );

      try {
        await expect(
          loadPdfDocument({ url: 'https://example.com/bad.pdf' }, 'https://example.com/bad.pdf')
        ).rejects.toThrow('Failed to load PDF document from https://example.com/bad.pdf');
      } finally {
        pdfjsLib.getDocument.mockReset();
        consoleErrorSpy.mockRestore();
      }
    });

    it('should propagate PdfError from resolvePath', async () => {
      const pdfError = new PdfError(ErrorCode.InvalidRequest, 'Path validation failed');
      pathUtils.resolvePath.mockImplementationOnce(() => {
        throw pdfError;
      });

      await expect(loadPdfDocument({ path: 'test.pdf' }, 'test.pdf')).rejects.toThrow(pdfError);
    });
  });
});
