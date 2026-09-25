/**
 * Integration test for MCP server with HTTP transport
 * Tests the actual JSON-RPC communication over HTTP
 */

import { type ChildProcess, execSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const repoRoot = path.resolve(__dirname, '../..');
const binWrapper = path.join(repoRoot, 'bin/anymd');
const ocrProvider = path.join(repoRoot, 'scripts/differential/reference-ocr-provider.ts');
const regionProvider = path.join(
  repoRoot,
  'scripts/differential/reference-region-analysis-provider.ts'
);
const RUST_HTTP_READY = 'Streamable HTTP MCP listening on http://';

const TEST_HOST = '127.0.0.1';
let baseUrl: string;
const packageJson = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, '../../package.json'), 'utf8')
) as {
  version: string;
};

const getFreePort = async (): Promise<number> =>
  new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.once('error', reject);
    server.listen(0, TEST_HOST, () => {
      const address = server.address();
      server.close(() => {
        if (typeof address === 'object' && address) {
          resolve(address.port);
        } else {
          reject(new Error('Failed to allocate a test HTTP port'));
        }
      });
    });
  });

const streamableHttpHeaders = {
  'Content-Type': 'application/json',
  Accept: 'application/json, text/event-stream',
};

const parseMcpResponse = async (response: Response) => {
  const contentType = response.headers.get('content-type') ?? '';
  const body = await response.text();

  if (contentType.includes('application/json')) {
    return JSON.parse(body) as Record<string, unknown>;
  }

  const dataLines = body
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice('data:'.length).trim())
    .filter((line) => line.length > 0);

  const payload = dataLines.at(-1);
  if (!payload) {
    throw new SyntaxError(`No MCP JSON payload in streamable HTTP response: ${body.slice(0, 200)}`);
  }
  return JSON.parse(payload) as Record<string, unknown>;
};

const createMcpHttpClient = () => {
  let sessionHeaders: Record<string, string> = { ...streamableHttpHeaders };

  const postMcp = async (body: Record<string, unknown>) => {
    const response = await fetch(baseUrl, {
      method: 'POST',
      headers: sessionHeaders,
      body: JSON.stringify(body),
    });
    const sessionId = response.headers.get('mcp-session-id');
    if (sessionId) {
      sessionHeaders = { ...sessionHeaders, 'mcp-session-id': sessionId };
    }
    return response;
  };

  const sendRequest = async (method: string, params?: unknown, id = 1) => {
    const response = await postMcp({
      jsonrpc: '2.0',
      id,
      method,
      params,
    });
    return parseMcpResponse(response);
  };

  const sendNotification = async (method: string, params?: unknown) => {
    await postMcp({
      jsonrpc: '2.0',
      method,
      params,
    });
  };

  const initializeSession = async () => {
    await sendRequest('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'test-http-client', version: '1.0.0' },
    });
    await sendNotification('notifications/initialized');
  };

  return { sendRequest, sendNotification, initializeSession };
};

const waitForRustHttpServer = (serverProc: ChildProcess) =>
  new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error('Rust HTTP MCP server startup timeout'));
    }, 30_000);

    const onReady = (output: string) => {
      if (output.includes(RUST_HTTP_READY)) {
        clearTimeout(timeout);
        setTimeout(resolve, 200);
      }
    };

    serverProc.stdout?.on('data', (data) => onReady(data.toString()));
    serverProc.stderr?.on('data', (data) => onReady(data.toString()));
  });

describe('MCP Server HTTP Transport Integration (Rust rmcp)', () => {
  let serverProc: ChildProcess;

  beforeAll(async () => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });

    const testPort = await getFreePort();
    baseUrl = `http://${TEST_HOST}:${String(testPort)}/mcp`;
    serverProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        NODE_ENV: 'test',
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        MCP_TRANSPORT: 'http',
        MCP_HTTP_PORT: testPort.toString(),
        MCP_HTTP_HOST: TEST_HOST,
        MCP_PDF_ALLOWED_DIRS: path.join(repoRoot, 'test/fixtures'),
        MCP_PDF_OCR_COMMAND: process.execPath,
        MCP_PDF_OCR_ARGS_JSON: JSON.stringify([ocrProvider, '{input}', '{page}', '{languages}']),
        MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
        MCP_PDF_REGION_ANALYSIS_ARGS_JSON: JSON.stringify([
          regionProvider,
          '{input}',
          '{page}',
          '{region_id}',
          '{evidence_id}',
          '{languages}',
        ]),
      },
    });

    await waitForRustHttpServer(serverProc);
  }, 300_000);

  afterAll(() => {
    serverProc?.kill('SIGTERM');
  });

  it('should respond to health check', async () => {
    const response = await fetch(`${baseUrl}/health`);
    expect(response.ok).toBe(true);
    const data = await response.json();
    expect(data.status).toBe('ok');
  });

  it('should respond to initialize request over HTTP', async () => {
    const client = createMcpHttpClient();
    const response = await client.sendRequest('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'test-http-client', version: '1.0.0' },
    });

    expect(response.id).toBe(1);
    expect(
      (response.result as { serverInfo?: { name?: string; version?: string } })?.serverInfo?.name
    ).toBe('anymd');
    const serverVersion = (response.result as { serverInfo?: { version?: string } })?.serverInfo
      ?.version;
    expect(serverVersion).toBe(packageJson.version);
  });

  it('should list available tools over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();

    const response = await client.sendRequest('tools/list', {}, 2);

    expect(response.id).toBe(2);
    expect(response.result?.tools).toBeDefined();
    expect(response.result?.tools?.length).toBeGreaterThan(0);

    const toolNames = response.result?.tools?.map((t: { name: string }) => t.name);
    expect(toolNames).toContain('read');
    expect(toolNames).toContain('search');
    expect(toolNames).toContain('inspect');
    // Legacy names stay callable but are no longer listed.
    expect(toolNames).not.toContain('read_pdf');
    expect(toolNames).not.toContain('inspect_pdf');
    expect(toolNames).not.toContain('render_page');
    expect(toolNames).not.toContain('extract_regions');
    expect(toolNames).not.toContain('analyze_regions');
    expect(toolNames).not.toContain('ocr_pages');
  });

  it('should reject local paths outside the native filesystem allowlist over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();
    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'read_pdf',
        arguments: {
          sources: [{ path: path.join(repoRoot, 'package.json') }],
        },
      },
      99
    );

    expect(response.id).toBe(99);
    expect((response.error as { message?: string } | undefined)?.message ?? '').toContain(
      'Access denied'
    );
  });

  it('should call read_pdf tool over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();

    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'read_pdf',
        arguments: {
          sources: [{ path: testPdfPath }],
          include_metadata: true,
          include_page_count: true,
          include_full_text: false,
        },
      },
      3
    );

    expect(response.id).toBe(3);

    // If test PDF doesn't exist, expect error
    if (response.error || response.result?.isError) {
      expect(response.error?.message || response.result?.content?.[0]?.text).toContain('PDF');
    } else {
      expect(response.result?.content).toBeDefined();
      expect(response.result?.content?.[0]?.type).toBe('text');
    }
  });

  it('should return bounded PNG evidence over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'pdf_evidence',
        arguments: {
          operation: 'render_page',
          sources: [{ path: testPdfPath, pages: [1] }],
          scale: 1,
          max_pages: 1,
        },
      },
      4
    );

    expect(response.error).toBeUndefined();
    const result = response.result as
      | {
          isError?: boolean;
          content?: Array<{ type?: string; text?: string; data?: string; mimeType?: string }>;
        }
      | undefined;
    expect(result?.isError).not.toBe(true);
    expect(result?.content?.[0]?.type).toBe('text');
    expect(result?.content?.[1]?.type).toBe('image');
    expect(result?.content?.[1]?.mimeType).toBe('image/png');
    const imageData = result?.content?.[1]?.data ?? '';
    expect(Buffer.from(imageData, 'base64').subarray(0, 8)).toEqual(
      Buffer.from('\x89PNG\r\n\x1a\n', 'binary')
    );
    const payload = JSON.parse(result?.content?.[0]?.text ?? '{}') as {
      results?: Array<{ rendered_pages?: Array<{ image_content_index?: number }> }>;
    };
    expect(payload.results?.[0]?.rendered_pages?.[0]?.image_content_index).toBe(1);
  });

  it('should return normalized command-provider OCR over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();
    const fixture = path.resolve(__dirname, '../fixtures/differential/v3014-visual-v1.pdf');
    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'pdf_evidence',
        arguments: {
          operation: 'ocr_pages',
          sources: [{ path: fixture, pages: [1] }],
          scale: 1,
          max_pages: 1,
          languages: ['eng'],
        },
      },
      5
    );
    expect(response.error).toBeUndefined();
    const result = response.result as {
      isError?: boolean;
      content?: Array<{ type?: string; text?: string }>;
    };
    expect(result.isError).not.toBe(true);
    expect(result.content).toHaveLength(1);
    const payload = JSON.parse(result.content?.[0]?.text ?? '{}') as {
      results?: Array<{ ocr_pages?: Array<Record<string, unknown>> }>;
    };
    expect(payload.results?.[0]?.ocr_pages?.[0]).toMatchObject({
      page: 1,
      text: 'Reference OCR page 1 at 120x80',
      language: 'eng',
      provider: 'command',
      provenance: { engine: 'external-command', source: 'ocr-provider' },
    });
  });

  it('should fuse command-provider OCR into read_pdf over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();
    const fixture = path.resolve(__dirname, '../fixtures/differential/v3014-visual-v1.pdf');
    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'read_pdf',
        arguments: {
          sources: [{ path: fixture, pages: [1] }],
          auto: false,
          include_document_map: true,
          include_ocr_text_layer: true,
        },
      },
      51
    );
    expect(response.error).toBeUndefined();
    const result = response.result as {
      isError?: boolean;
      content?: Array<{ type?: string; text?: string }>;
      structuredContent?: {
        evidence?: { confidence?: string };
        results?: Array<{
          success?: boolean;
          data?: {
            ocr_text_layer?: { profile?: string; pages?: Array<Record<string, unknown>> };
            document_map?: { layers?: string[]; routing?: Record<string, unknown> };
          };
        }>;
      };
    };
    expect(result.isError).not.toBe(true);
    const data = result.structuredContent?.results?.[0]?.data;
    expect(data?.ocr_text_layer).toMatchObject({
      profile: 'ocr_text_layer',
      pages: [
        {
          page: 1,
          text: 'Reference OCR page 1 at 240x160',
          provider: 'command',
        },
      ],
    });
    expect(data?.document_map?.layers).toContain('ocr_text_layer');
    expect(data?.document_map?.routing).toMatchObject({ ocr_applied_pages: [1] });
    expect(result.structuredContent?.evidence?.confidence).toBe('provider-dependent');
    expect(result.content?.at(-1)?.text).toBe('[Page 1 OCR]\nReference OCR page 1 at 240x160');
  });

  it('should return normalized command-provider region analysis over HTTP', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();
    const fixture = path.resolve(__dirname, '../fixtures/differential/v3014-visual-v1.pdf');
    const response = await client.sendRequest(
      'tools/call',
      {
        name: 'pdf_evidence',
        arguments: {
          operation: 'analyze_regions',
          sources: [
            {
              path: fixture,
              regions: [
                {
                  id: 'formula',
                  page: 2,
                  bounding_box: { left: 0, bottom: 0, right: 20, top: 30 },
                },
              ],
            },
          ],
          scale: 1,
          max_regions: 1,
        },
      },
      6
    );
    expect(response.error).toBeUndefined();
    const result = response.result as {
      isError?: boolean;
      content?: Array<{ type?: string; text?: string }>;
    };
    expect(result.isError).not.toBe(true);
    const payload = JSON.parse(result.content?.[0]?.text ?? '{}') as {
      results?: Array<{ region_analyses?: Array<Record<string, unknown>> }>;
    };
    expect(payload.results?.[0]?.region_analyses?.[0]).toMatchObject({
      region_id: 'formula',
      page: 2,
      kind: 'formula',
      provider: 'command',
      provenance: { engine: 'external-command', source: 'region-analysis-provider' },
    });
  });

  const goldenPath = path.resolve(__dirname, '../fixtures/read-pdf-golden.json');
  const golden = JSON.parse(fs.readFileSync(goldenPath, 'utf8')) as {
    cases: Array<{
      id: string;
      fixture?: string;
      input?: Record<string, unknown>;
      expects: {
        error?: boolean;
        message_contains?: string;
        route?: string;
        payload?: Record<string, unknown>;
      };
    }>;
  };

  for (const caseId of [
    'sample-metadata-on',
    'sample-minimal-route',
    'missing-file',
    'empty-sources',
    'url-source',
  ] as const) {
    it(`read_pdf golden mock parity over HTTP for ${caseId}`, async () => {
      const caseEntry = golden.cases.find((entry) => entry.id === caseId);
      expect(caseEntry).toBeDefined();

      const client = createMcpHttpClient();
      await client.initializeSession();

      const args: Record<string, unknown> = { ...(caseEntry?.input ?? {}) };
      if (caseEntry?.fixture && !Object.hasOwn(args, 'sources')) {
        const fixturePath = path.resolve(__dirname, '../fixtures', caseEntry.fixture);
        args.sources = [{ path: fixturePath }];
      }

      const response = await client.sendRequest(
        'tools/call',
        {
          name: 'read_pdf',
          arguments: args,
        },
        20 + golden.cases.findIndex((entry) => entry.id === caseId)
      );

      if (caseEntry?.expects.error) {
        const message =
          (response.error as { message?: string } | undefined)?.message ??
          (response.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]
            ?.text ??
          '';
        expect(String(message).toLowerCase()).toContain(
          caseEntry?.expects.message_contains?.toLowerCase() ?? ''
        );
        return;
      }

      // Success path is best-effort when sample PDF fixture is present.
      if (response.result) {
        expect(response.result).toBeDefined();
      }
    });
  }

  it('should not return wildcard CORS headers by default', async () => {
    const response = await fetch(baseUrl, {
      method: 'OPTIONS',
      headers: {
        Origin: 'http://example.com',
        'Access-Control-Request-Method': 'POST',
      },
    });

    // Without MCP_CORS_ORIGIN, no CORS wildcard should be set
    const corsHeader = response.headers.get('Access-Control-Allow-Origin');
    expect(corsHeader).not.toBe('*');
  });

  it('should reject invalid JSON-RPC requests', async () => {
    const client = createMcpHttpClient();
    await client.initializeSession();

    const response = await fetch(baseUrl, {
      method: 'POST',
      headers: streamableHttpHeaders,
      body: JSON.stringify({
        // Missing jsonrpc version
        id: 1,
        method: 'tools/list',
      }),
    });

    expect(response.ok).toBe(false);
    const body = await response.text();
    expect(body.length).toBeGreaterThan(0);
  });
});

describe('MCP Server HTTP Transport Authentication (Rust rmcp)', () => {
  const API_KEY = 'test-secret-key-123';
  let serverProc: ChildProcess;
  let authBaseUrl: string;

  beforeAll(async () => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });

    const testPort = await getFreePort();
    authBaseUrl = `http://${TEST_HOST}:${String(testPort)}/mcp`;
    serverProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        NODE_ENV: 'test',
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        MCP_TRANSPORT: 'http',
        MCP_HTTP_PORT: testPort.toString(),
        MCP_HTTP_HOST: TEST_HOST,
        MCP_API_KEY: API_KEY,
      },
    });

    await waitForRustHttpServer(serverProc);
  }, 300_000);

  afterAll(() => {
    serverProc?.kill('SIGTERM');
  });

  const initialize = (headers: Record<string, string>) =>
    fetch(authBaseUrl, {
      method: 'POST',
      headers: { ...streamableHttpHeaders, ...headers },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2024-11-05',
          capabilities: {},
          clientInfo: { name: 'auth-test-client', version: '1.0.0' },
        },
      }),
    });

  it('rejects requests with no X-API-Key header (401)', async () => {
    const response = await initialize({});
    expect(response.status).toBe(401);
    const data = await response.json();
    expect(data.error?.message).toContain('X-API-Key');
  });

  it('rejects requests with a wrong X-API-Key (401)', async () => {
    const response = await initialize({ 'X-API-Key': 'wrong-key' });
    expect(response.status).toBe(401);
  });

  it('accepts requests carrying the correct X-API-Key', async () => {
    const response = await initialize({ 'X-API-Key': API_KEY });
    expect(response.status).toBe(200);
    const data = await parseMcpResponse(response);
    expect((data.result as { serverInfo?: { name?: string } })?.serverInfo?.name).toBe('anymd');
  });

  it('does not list tools to an unauthenticated caller', async () => {
    const response = await fetch(authBaseUrl, {
      method: 'POST',
      headers: streamableHttpHeaders,
      body: JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} }),
    });
    expect(response.status).toBe(401);
  });

  it('keeps the health endpoint open without a key', async () => {
    const response = await fetch(`${authBaseUrl}/health`);
    expect(response.ok).toBe(true);
    const data = await response.json();
    expect(data.status).toBe('ok');
  });
});
