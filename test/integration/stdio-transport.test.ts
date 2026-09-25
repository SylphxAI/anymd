/**
 * Integration test for MCP server with opt-in Rust rmcp stdio transport.
 * Tests JSON-RPC communication over stdio and golden read_pdf parity.
 *
 * Production default is TypeScript (dist/index.js). These cases prove the
 * experimental Rust engine path via PDF_READER_MCP_ENGINE=rust.
 */

import { type ChildProcess, execSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const repoRoot = path.resolve(__dirname, '../..');
const binWrapper = path.join(repoRoot, 'bin/anymd');
const fixturesRoot = path.join(repoRoot, 'test/fixtures');
const goldenPath = path.join(fixturesRoot, 'read-pdf-golden.json');
const samplePdf = path.join(fixturesRoot, 'sample.pdf');

type GoldenCase = {
  id: string;
  fixture: string;
  input: Record<string, unknown>;
  expects: {
    error?: boolean;
    code?: string;
    message_contains?: string;
    route?: string;
    payload?: Record<string, unknown>;
  };
};

type GoldenManifest = {
  profile: string;
  cases: GoldenCase[];
};

const createRequest = (id: number, method: string, params?: unknown) => ({
  jsonrpc: '2.0' as const,
  id,
  method,
  params,
});

const sendMessage = (proc: ChildProcess, message: object): void => {
  const json = JSON.stringify(message);
  proc.stdin?.write(`${json}\n`);
};

const readResponse = (proc: ChildProcess, timeout = 30_000): Promise<unknown> =>
  new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => {
      reject(new Error(`Timeout waiting for stdio response. Buffer: ${buffer}`));
    }, timeout);

    const onData = (data: Buffer) => {
      buffer += data.toString();
      const newlineIndex = buffer.indexOf('\n');
      if (newlineIndex === -1) {
        return;
      }

      const line = buffer.slice(0, newlineIndex).trim();
      buffer = buffer.slice(newlineIndex + 1);
      if (line.length === 0) {
        return;
      }

      clearTimeout(timer);
      proc.stdout?.off('data', onData);
      try {
        resolve(JSON.parse(line));
      } catch {
        reject(new Error(`Failed to parse JSON: ${line}`));
      }
    };

    proc.stdout?.on('data', onData);
  });

const normalizePayload = (payload: Record<string, unknown>) => {
  const normalized = structuredClone(payload);
  if (Array.isArray(normalized.results)) {
    normalized.results = normalized.results.map((result) => {
      if (!result || typeof result !== 'object') {
        return result;
      }
      const entry = { ...(result as Record<string, unknown>) };
      if (typeof entry.source === 'string') {
        entry.source = path.relative(fixturesRoot, entry.source).split(path.sep).join('/');
      }
      if (entry.data && typeof entry.data === 'object') {
        const data = { ...(entry.data as Record<string, unknown>) };
        data.fullText = undefined;
        data.full_text = undefined;
        data.evidence = undefined;
        if (data.info && typeof data.info === 'object') {
          const info = { ...(data.info as Record<string, unknown>) };
          info.text_chars = undefined;
          info.textChars = undefined;
          data.info = info;
        }
        entry.data = data;
      }
      return entry;
    });
  }
  normalized.evidence = undefined;
  return normalized;
};

const buildRequestInput = (fixture: string, input: Record<string, unknown>) => {
  const request = structuredClone(input);
  if (!Array.isArray(request.sources)) {
    request.sources = [{ path: path.join(fixturesRoot, fixture) }];
  } else {
    request.sources = request.sources.map((source) => {
      if (!source || typeof source !== 'object') {
        return source;
      }
      const entry = { ...(source as Record<string, unknown>) };
      if (typeof entry.path === 'string' && !path.isAbsolute(entry.path)) {
        entry.path = path.join(fixturesRoot, entry.path);
      }
      return entry;
    });
  }
  return request;
};

const parseStructuredContent = (result: Record<string, unknown> | undefined) => {
  if (!result) {
    return undefined;
  }
  if (result.structuredContent && typeof result.structuredContent === 'object') {
    return result.structuredContent as Record<string, unknown>;
  }
  const text = (result.content as Array<{ text?: string }> | undefined)?.[0]?.text;
  return text ? (JSON.parse(text) as Record<string, unknown>) : undefined;
};

const initializeSession = async (proc: ChildProcess) => {
  sendMessage(
    proc,
    createRequest(1, 'initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'test-stdio-client', version: '1.0.0' },
    })
  );
  await readResponse(proc);
  sendMessage(proc, { jsonrpc: '2.0', method: 'notifications/initialized' });
  await new Promise((resolve) => setTimeout(resolve, 100));
};

const packageJson = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, '../../package.json'), 'utf8')
) as {
  version: string;
};

describe('MCP Server stdio Transport Integration (Rust rmcp)', () => {
  let serverProc: ChildProcess;

  beforeAll(async () => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });

    serverProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        NODE_ENV: 'test',
        PDF_READER_MCP_TRANSPORT: '',
        MCP_TRANSPORT: '',
      },
    });

    await new Promise((resolve) => setTimeout(resolve, 1500));
  }, 300_000);

  afterAll(() => {
    serverProc?.kill('SIGTERM');
  });

  it('should respond to initialize request over stdio', async () => {
    const freshProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        NODE_ENV: 'test',
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 1000));

    sendMessage(
      freshProc,
      createRequest(101, 'initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'test-stdio-init', version: '1.0.0' },
      })
    );
    const response = (await readResponse(freshProc)) as {
      id: number;
      result?: { serverInfo?: { name?: string; version?: string } };
    };

    expect(response.id).toBe(101);
    expect(response.result?.serverInfo?.name).toBe('anymd');
    expect(response.result?.serverInfo?.version).toBe(packageJson.version);
    // sole-runtime may advertise package version
    freshProc.kill('SIGTERM');
  });

  it('should list available tools over stdio', async () => {
    await initializeSession(serverProc);

    sendMessage(serverProc, createRequest(2, 'tools/list', {}));
    const response = (await readResponse(serverProc)) as {
      id: number;
      result?: { tools?: Array<{ name: string }> };
    };

    expect(response.id).toBe(2);
    expect(response.result?.tools).toBeDefined();
    expect(response.result?.tools?.length).toBeGreaterThan(0);

    const toolNames = response.result?.tools?.map((tool) => tool.name);
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

  const golden = JSON.parse(fs.readFileSync(goldenPath, 'utf8')) as GoldenManifest;

  for (const caseId of [
    'sample-metadata-on',
    'sample-minimal-route',
    'missing-file',
    'empty-sources',
    'url-source',
  ] as const) {
    it(`read_pdf golden mock parity over stdio for ${caseId}`, async () => {
      const caseEntry = golden.cases.find((entry) => entry.id === caseId);
      expect(caseEntry).toBeDefined();

      const freshProc = spawn(binWrapper, [], {
        stdio: ['pipe', 'pipe', 'pipe'],
        env: {
          ...process.env,
          PDF_READER_MCP_ENGINE: 'rust',
          PDF_READER_ENGINE_MODE: 'pure-rust',
          NODE_ENV: 'test',
        },
      });
      await new Promise((resolve) => setTimeout(resolve, 1000));
      await initializeSession(freshProc);

      sendMessage(
        freshProc,
        createRequest(10 + golden.cases.findIndex((entry) => entry.id === caseId), 'tools/call', {
          name: 'read_pdf',
          arguments: buildRequestInput(caseEntry?.fixture, caseEntry?.input),
        })
      );
      const response = (await readResponse(freshProc, 60_000)) as {
        error?: { message?: string };
        result?: Record<string, unknown>;
      };

      if (caseEntry?.expects.error) {
        const message =
          response.error?.message ??
          (response.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]
            ?.text ??
          '';
        expect(message.toLowerCase()).toContain(
          caseEntry?.expects.message_contains?.toLowerCase() ?? ''
        );
        freshProc.kill('SIGTERM');
        return;
      }

      if (!fs.existsSync(samplePdf)) {
        freshProc.kill('SIGTERM');
        return;
      }

      const structured = parseStructuredContent(response.result);
      expect(structured).toBeDefined();

      const actual = normalizePayload(structured as Record<string, unknown>);
      const expected = normalizePayload(caseEntry?.expects.payload as Record<string, unknown>);

      expect(actual.profile).toBe(expected.profile);
      const actualResults = actual.results as Array<Record<string, unknown>>;
      const expectedResults = expected.results as Array<Record<string, unknown>>;
      expect(actualResults[0]?.success).toBe(expectedResults[0]?.success);
      expect((actualResults[0]?.data as Record<string, unknown> | undefined)?.route).toBe(
        caseEntry?.expects.route
      );
      expect(
        (actualResults[0]?.data as { engine?: { name?: string; version?: string } } | undefined)
          ?.engine
      ).toEqual(
        (expectedResults[0]?.data as { engine?: { name?: string; version?: string } } | undefined)
          ?.engine
      );

      const actualInfo = (actualResults[0]?.data as { info?: Record<string, unknown> } | undefined)
        ?.info;
      const expectedInfo = (
        expectedResults[0]?.data as { info?: Record<string, unknown> } | undefined
      )?.info;
      if (expectedInfo) {
        for (const [key, value] of Object.entries(expectedInfo)) {
          expect(actualInfo?.[key]).toEqual(value);
        }
      }

      freshProc.kill('SIGTERM');
    });
  }

  it('should call search_pdf over stdio with rust-text-index route', async () => {
    if (!fs.existsSync(samplePdf)) {
      return;
    }

    const freshProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        NODE_ENV: 'test',
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 1000));
    await initializeSession(freshProc);

    sendMessage(
      freshProc,
      createRequest(20, 'tools/call', {
        name: 'search_pdf',
        arguments: {
          sources: [{ path: samplePdf }],
          query: 'Lorem',
          detail: true,
        },
      })
    );
    const response = (await readResponse(freshProc, 60_000)) as {
      result?: { content?: Array<{ text?: string }> };
    };

    const textContent = response.result?.content?.[0]?.text ?? '';
    const parsed = JSON.parse(textContent) as {
      profile: string;
      results: Array<{ success: boolean }>;
    };
    expect(parsed.profile).toBe('pdf_search_results');
    expect(parsed.results[0]?.success).toBe(true);
    expect(textContent).toContain('rust-text-index');
    freshProc.kill('SIGTERM');
  });

  it('should call pdf_evidence inspect over stdio with rust-pdf-inspect-v1 route', async () => {
    if (!fs.existsSync(samplePdf)) {
      return;
    }

    const freshProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        PDF_READER_MCP_ENGINE: 'rust',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        NODE_ENV: 'test',
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 1000));
    await initializeSession(freshProc);

    sendMessage(
      freshProc,
      createRequest(21, 'tools/call', {
        name: 'pdf_evidence',
        arguments: {
          operation: 'inspect',
          sources: [{ path: samplePdf }],
        },
      })
    );
    const response = (await readResponse(freshProc, 60_000)) as {
      result?: { content?: Array<{ text?: string }> };
    };

    const textContent = response.result?.content?.[0]?.text ?? '';
    expect(textContent).toContain('rust-pdf-inspect-v1');
    expect(textContent).toContain('"success":true');
    freshProc.kill('SIGTERM');
  });
});
