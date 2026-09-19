// PDF document loading utilities

import dns from 'node:dns';
import fs from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
import { createRequire } from 'node:module';
import net from 'node:net';
import type * as pdfjsLib from 'pdfjs-dist/legacy/build/pdf.mjs';
import { getDocument } from 'pdfjs-dist/legacy/build/pdf.mjs';
import {
  assertUrlNotPrivate,
  getSecurityConfig,
  isPrivateIp,
  isUrlAllowed,
  type SecurityConfig,
} from '../utils/config.js';
import { ErrorCode, PdfError } from '../utils/errors.js';
import { createLogger } from '../utils/logger.js';
import { resolvePath } from '../utils/pathUtils.js';
import { destroyLoadingTask } from '../utils/pdfjs.js';
import type { PdfSessionScope } from './pdfSession.js';

const logger = createLogger('Loader');

// Resolve pdfjs-dist resource paths relative to the installed package.
//
// pdfjs-dist ships data files (CMaps, standard fonts, WASM decoders, ICC
// profiles) alongside its code. In Node.js these are loaded from the local
// filesystem, so each URL must be an absolute path to the directory — with a
// trailing slash — regardless of the current working directory.
//
// Why all four URLs matter:
//   - cMapUrl:            predefined Adobe CMaps for CJK/special encodings
//   - standardFontDataUrl: PDF standard fonts (Helvetica, Times, etc.)
//   - wasmUrl:            OpenJPEG (JPEG 2000) + QCMS (color management) WASM
//   - iccUrl:             ICC color profiles
//
// If any URL is omitted, pdfjs-dist logs "Ensure that the `<name>` API
// parameter is provided" and — worse — some code paths concatenate `null`
// with the filename (e.g. `nullopenjpeg_nowasm_fallback.js`), producing an
// ERR_MODULE_NOT_FOUND that breaks image decoding entirely (issue #271).
const require = createRequire(import.meta.url);
const PDFJS_ROOT = require.resolve('pdfjs-dist/package.json').replace('package.json', '');
const CMAP_URL = `${PDFJS_ROOT}cmaps/`;
const STANDARD_FONT_DATA_URL = `${PDFJS_ROOT}standard_fonts/`;
const WASM_URL = `${PDFJS_ROOT}wasm/`;
const ICC_URL = `${PDFJS_ROOT}iccs/`;

// Maximum PDF file size: 100MB
// Prevents memory exhaustion from loading extremely large files
const MAX_PDF_SIZE = 100 * 1024 * 1024;

// Per-request timeout for URL fetches. Bounds how long a slow or hostile
// server can keep the event loop tied up (SSS-08).
const URL_FETCH_TIMEOUT_MS = 30_000;

// Maximum redirect hops we will follow when fetching a URL. Each hop is
// re-validated against the SSRF policy.
const MAX_REDIRECTS = 5;

const formatBytes = (bytes: number): string => `${(bytes / 1024 / 1024).toFixed(0)}MB`;

const sanitizeSourceDescription = (description: string): string =>
  description.length > 200 ? `${description.slice(0, 197)}...` : description;

/**
 * Read a local PDF file into memory, refusing oversized files before they are
 * fully buffered (SSS-08). `fs.stat` runs first so a multi-gigabyte file is
 * rejected without ever touching the page cache.
 */
const loadLocalFile = async (userPath: string): Promise<Uint8Array> => {
  const safePath = resolvePath(userPath);

  let stats: Awaited<ReturnType<typeof fs.stat>>;
  try {
    stats = await fs.stat(safePath);
  } catch (err: unknown) {
    if (typeof err === 'object' && err !== null && 'code' in err && err.code === 'ENOENT') {
      throw new PdfError(ErrorCode.InvalidRequest, `File not found at '${userPath}'.`, {
        cause: err instanceof Error ? err : undefined,
      });
    }
    throw new PdfError(ErrorCode.InvalidRequest, `Failed to access file at '${userPath}'.`, {
      cause: err instanceof Error ? err : undefined,
    });
  }

  if (!stats.isFile()) {
    throw new PdfError(ErrorCode.InvalidRequest, `Path '${userPath}' is not a regular file.`);
  }

  if (stats.size > MAX_PDF_SIZE) {
    throw new PdfError(
      ErrorCode.InvalidRequest,
      `PDF file exceeds maximum size of ${formatBytes(MAX_PDF_SIZE)}. File size: ${formatBytes(stats.size)}.`
    );
  }

  const buffer = await fs.readFile(safePath);
  return new Uint8Array(buffer);
};

/**
 * Validate one URL hop against the configured policy plus SSRF guard
 * (SSS-07). Returns silently when the URL is acceptable; throws PdfError on
 * any policy/SSRF violation.
 */
const validateUrlHop = async (urlString: string, config: SecurityConfig): Promise<void> => {
  if (!isUrlAllowed(urlString, config)) {
    const reason = config.allowHttp
      ? 'host is not in the allowed list or scheme is not http(s)'
      : 'HTTP access is disabled';
    throw new PdfError(
      ErrorCode.InvalidRequest,
      `Access denied: URL '${urlString}' rejected (${reason}).`
    );
  }

  if (!config.allowPrivateIps) {
    let hostname: string;
    try {
      hostname = new URL(urlString).hostname;
    } catch {
      throw new PdfError(ErrorCode.InvalidRequest, `Invalid URL: '${urlString}'.`);
    }
    try {
      await assertUrlNotPrivate(hostname);
    } catch (err) {
      const reason = err instanceof Error ? err.message : 'SSRF check failed';
      throw new PdfError(ErrorCode.InvalidRequest, `Access denied: ${reason}`);
    }
  }
};

/**
 * Build the HTTP(S) agent that pins one hop's connection to the addresses the
 * SSRF guard already approved.
 *
 * Without this the guard only *checks* the hostname: the client would resolve
 * it again when it connects, so an attacker-controlled zone can answer the
 * check with a public address and the connect with `169.254.169.254` — the
 * standard DNS-rebinding (TOCTOU) bypass (GHSA-5r2f-7788-qp8v).
 *
 * The pinned `lookup` replaces socket-level resolution, so the address that was
 * validated is the address dialed, on every hop. Host and TLS SNI still come
 * from the URL, so virtual hosting and certificate validation are unaffected.
 * A literal-IP host needs no pinning (it is already the connection target).
 */
type ResolvedAddress = { address: string; family: number };

export const createPinnedAgent = async (urlString: string): Promise<http.Agent | https.Agent> => {
  const parsed = new URL(urlString);
  const hostname = parsed.hostname;
  const isHttps = parsed.protocol === 'https:';

  // A literal IP is already the connection target; nothing can rebind it.
  if (net.isIP(hostname)) {
    return isHttps ? new https.Agent({ keepAlive: false }) : new http.Agent({ keepAlive: false });
  }

  let addresses: dns.LookupAddress[];
  try {
    addresses = await dns.promises.lookup(hostname, { all: true });
  } catch {
    throw new PdfError(ErrorCode.InvalidRequest, `URL host '${hostname}' could not be resolved.`);
  }
  if (addresses.length === 0) {
    throw new PdfError(
      ErrorCode.InvalidRequest,
      `URL host '${hostname}' resolved to no addresses.`
    );
  }

  // Fail closed on any non-public answer. This repeats the guard's predicate on
  // the same answer, so a hostile zone cannot satisfy the check and then
  // re-answer at connect time.
  const approved = addresses.filter(({ address }) => {
    if (isPrivateIp(address)) {
      throw new PdfError(
        ErrorCode.InvalidRequest,
        `Access denied: URL host '${hostname}' resolves to a non-public address (SSRF protection).`
      );
    }
    return true;
  });

  const lookup = (
    _lookupHostname: string,
    options: dns.LookupOptions | ((...args: unknown[]) => void),
    callback?: (...args: unknown[]) => void
  ): void => {
    const done = (typeof options === 'function' ? options : callback) as (
      err: NodeJS.ErrnoException | null,
      address: string | dns.LookupAddress[],
      family?: number
    ) => void;
    if (typeof options !== 'object' || options === null) {
      // Defensive: an unexpected calling convention must fail closed rather
      // than silently hand back an unpinned answer.
      const err: NodeJS.ErrnoException = new Error(
        `URL host '${hostname}' could not be pinned (unexpected resolver call).`
      );
      err.code = 'EINVAL';
      done(err, '');
      return;
    }
    // Node 18+ passes `{ all: true }` for HTTP(S) connections. Honour it, or the
    // client re-resolves the name and the pinned answer never reaches the socket.
    if (options.all === true) {
      done(null, approved);
      return;
    }
    const first = approved[0] as ResolvedAddress;
    done(null, first.address, first.family);
  };

  // The runtime contract is dns.lookup-shaped; Node's agent types expect the
  // node:dns `LookupFunction` shape, which this satisfies.
  const agentLookup = lookup as unknown as net.LookupFunction;
  return isHttps
    ? new https.Agent({ lookup: agentLookup, keepAlive: false })
    : new http.Agent({ lookup: agentLookup, keepAlive: false });
};

/**
 * Perform one hop through an agent whose resolver is pinned, so the address the
 * SSRF guard approved is the address the socket connects to.
 */
const fetchThroughAgent = async (
  urlString: string,
  init: RequestInit,
  agent: http.Agent | https.Agent
): Promise<Response> => {
  const parsed = new URL(urlString);
  const requestFn = parsed.protocol === 'https:' ? https.request : http.request;
  return await new Promise<Response>((resolve, reject) => {
    const req = requestFn(urlString, { agent, method: 'GET' }, (res) => {
      const chunks: Buffer[] = [];
      res.on('data', (chunk: Buffer) => chunks.push(chunk));
      res.on('end', () => {
        const headers = new Headers();
        for (const [key, value] of Object.entries(res.headers)) {
          if (typeof value === 'string') headers.set(key, value);
          else if (Array.isArray(value)) headers.set(key, value.join(', '));
        }
        resolve(
          new Response(Buffer.concat(chunks), {
            status: res.statusCode ?? 502,
            statusText: res.statusMessage ?? '',
            headers,
          })
        );
      });
    });
    req.on('error', reject);
    // `redirect: 'manual'` semantics: a 3xx response is returned, not followed.
    if (init.signal) init.signal.addEventListener('abort', () => req.destroy(), { once: true });
    req.end();
  });
};

/**
 * Test seam for the socket I/O of one hop. The pin always runs; tests replace
 * only the transport so policy, redirect, and size handling can be exercised
 * without opening a socket.
 */
let fetchUrlHopForTests: ((url: string, init: RequestInit) => Promise<Response>) | null = null;

export const __setFetchUrlHopForTests = (
  impl: ((url: string, init: RequestInit) => Promise<Response>) | null
): void => {
  fetchUrlHopForTests = impl;
};

const fetchUrlHop = async (
  urlString: string,
  init: RequestInit,
  config: SecurityConfig
): Promise<Response> => {
  if (config.allowPrivateIps) {
    const init_ = init.signal
      ? { redirect: 'manual', signal: init.signal }
      : { redirect: 'manual' };
    return fetch(urlString, init_ as RequestInit);
  }
  // The pin always runs: it is the security control, not an implementation
  // detail the transport can skip. Tests substitute only the socket I/O.
  const agent = await createPinnedAgent(urlString);
  try {
    if (fetchUrlHopForTests) return await fetchUrlHopForTests(urlString, init);
    return await fetchThroughAgent(urlString, init, agent);
  } finally {
    agent.destroy();
  }
};

/**
 * Fetch a PDF from `url` with SSRF protection, redirect re-validation, an
 * overall timeout, and a streaming size cap (SSS-07 + SSS-08). Returns the
 * full body as a Uint8Array so we can hand it to PDF.js as `data:` and keep
 * resource limits enforced at the application layer.
 */
const fetchUrlBody = async (url: string, config: SecurityConfig): Promise<Uint8Array> => {
  let currentUrl = url;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), URL_FETCH_TIMEOUT_MS);

  try {
    for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
      await validateUrlHop(currentUrl, config);

      // Pin at the connection boundary: the address validated above is the
      // address dialed, and a second DNS answer cannot redirect the socket
      // (GHSA-5r2f-7788-qp8v). `allowPrivateIps` keeps the plain fetch path so
      // the documented opt-in for local fixtures still works.
      const response = await fetchUrlHop(currentUrl, { signal: controller.signal }, config);

      if (response.status >= 300 && response.status < 400) {
        const location = response.headers.get('location');
        if (!location) {
          throw new PdfError(
            ErrorCode.InvalidRequest,
            `URL fetch failed: redirect without Location header.`
          );
        }
        currentUrl = new URL(location, currentUrl).toString();
        continue;
      }

      if (!response.ok) {
        throw new PdfError(
          ErrorCode.InvalidRequest,
          `URL fetch failed with HTTP ${String(response.status)}.`
        );
      }

      const contentLengthHeader = response.headers.get('content-length');
      if (contentLengthHeader !== null) {
        const declared = Number.parseInt(contentLengthHeader, 10);
        if (Number.isFinite(declared) && declared > MAX_PDF_SIZE) {
          throw new PdfError(
            ErrorCode.InvalidRequest,
            `Remote PDF exceeds maximum size of ${formatBytes(MAX_PDF_SIZE)} (Content-Length: ${formatBytes(declared)}).`
          );
        }
      }

      if (!response.body) {
        // No streaming body (some implementations); fall back to arrayBuffer
        // but still apply the size cap.
        const ab = await response.arrayBuffer();
        if (ab.byteLength > MAX_PDF_SIZE) {
          throw new PdfError(
            ErrorCode.InvalidRequest,
            `Remote PDF exceeds maximum size of ${formatBytes(MAX_PDF_SIZE)}.`
          );
        }
        return new Uint8Array(ab);
      }

      const reader = response.body.getReader();
      const chunks: Uint8Array[] = [];
      let total = 0;
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        if (value) {
          total += value.byteLength;
          if (total > MAX_PDF_SIZE) {
            await reader.cancel().catch(() => {});
            throw new PdfError(
              ErrorCode.InvalidRequest,
              `Remote PDF exceeds maximum size of ${formatBytes(MAX_PDF_SIZE)} during streaming.`
            );
          }
          chunks.push(value);
        }
      }

      const combined = new Uint8Array(total);
      let offset = 0;
      for (const chunk of chunks) {
        combined.set(chunk, offset);
        offset += chunk.byteLength;
      }
      return combined;
    }

    throw new PdfError(
      ErrorCode.InvalidRequest,
      `URL fetch failed: exceeded redirect limit (${String(MAX_REDIRECTS)}).`
    );
  } catch (err: unknown) {
    if (err instanceof PdfError) throw err;
    if (err instanceof Error && (err.name === 'AbortError' || err.name === 'TimeoutError')) {
      throw new PdfError(
        ErrorCode.InvalidRequest,
        `URL fetch timed out after ${String(URL_FETCH_TIMEOUT_MS / 1000)}s.`,
        { cause: err }
      );
    }
    const message = err instanceof Error ? err.message : String(err);
    logger.warn('URL fetch failed', { url, error: message });
    throw new PdfError(ErrorCode.InvalidRequest, `URL fetch failed for '${url}'.`, {
      cause: err instanceof Error ? err : undefined,
    });
  } finally {
    clearTimeout(timeout);
  }
};

/**
 * Load a PDF document from a local file path or URL
 * @param source - Object containing either path or url
 * @param sourceDescription - Description for error messages
 * @returns PDF document proxy
 */
export const loadPdfDocumentCore = async (
  source: { path?: string | undefined; url?: string | undefined },
  sourceDescription: string
): Promise<pdfjsLib.PDFDocumentProxy> => {
  const safeSource = sanitizeSourceDescription(sourceDescription);
  let pdfData: Uint8Array;

  try {
    if (source.path) {
      pdfData = await loadLocalFile(source.path);
    } else if (source.url) {
      const config = getSecurityConfig();
      pdfData = await fetchUrlBody(source.url, config);
    } else {
      throw new PdfError(ErrorCode.InvalidParams, `Source ${safeSource} missing 'path' or 'url'.`);
    }
  } catch (err: unknown) {
    if (err instanceof PdfError) {
      throw err;
    }

    // Non-PdfError exceptions are logged with full detail but only surface a
    // generic message to callers — raw filesystem/library messages can leak
    // internal paths back to the LLM (SSS-02).
    const message = err instanceof Error ? err.message : String(err);
    logger.error('Unexpected error preparing PDF source', {
      sourceDescription: safeSource,
      error: message,
    });
    throw new PdfError(ErrorCode.InvalidRequest, `Failed to prepare PDF source ${safeSource}.`, {
      cause: err instanceof Error ? err : undefined,
    });
  }

  const loadingTask = getDocument({
    data: pdfData,
    cMapUrl: CMAP_URL,
    cMapPacked: true,
    standardFontDataUrl: STANDARD_FONT_DATA_URL,
    wasmUrl: WASM_URL,
    iccUrl: ICC_URL,
  });

  try {
    return await loadingTask.promise;
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    logger.error('PDF.js loading error', { sourceDescription: safeSource, error: message });
    throw new PdfError(
      ErrorCode.InvalidRequest,
      `Failed to load PDF document from ${safeSource}.`,
      { cause: err instanceof Error ? err : undefined }
    );
  }
};

/**
 * Load a PDF document, reusing a parsed handle from `session` when provided.
 */
export const loadPdfDocument = async (
  source: { path?: string | undefined; url?: string | undefined },
  sourceDescription: string,
  session?: PdfSessionScope
): Promise<pdfjsLib.PDFDocumentProxy> => {
  if (session) {
    return session.acquire(source, sourceDescription);
  }
  return loadPdfDocumentCore(source, sourceDescription);
};

/**
 * Release a PDF document acquired through `session`, or destroy a standalone load.
 */
export const releasePdfDocument = async (
  source: { path?: string | undefined; url?: string | undefined },
  pdfDocument: pdfjsLib.PDFDocumentProxy | null,
  sourceDescription: string,
  session?: PdfSessionScope
): Promise<void> => {
  if (session) {
    session.release(source);
    return;
  }
  await destroyLoadingTask(pdfDocument?.loadingTask, logger, 'PDF document', {
    sourceDescription,
  });
};
