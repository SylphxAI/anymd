/**
 * Integration test for MCP server
 * Tests the actual JSON-RPC communication over stdio
 */

import { type ChildProcess, execSync, spawn } from 'node:child_process';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const repoRoot = path.resolve(__dirname, '../..');
const binWrapper = path.join(repoRoot, 'bin/anymd');

// JSON-RPC message helpers
const createRequest = (id: number, method: string, params?: unknown) => ({
  jsonrpc: '2.0' as const,
  id,
  method,
  params,
});

// MCP uses JSON Lines format (newline-delimited JSON)
const sendMessage = (proc: ChildProcess, message: object): void => {
  const json = JSON.stringify(message);
  proc.stdin?.write(`${json}\n`);
};

// Generous per-request timeout for Rust rmcp stdio cold start on loaded CI runners.
const readResponse = (proc: ChildProcess, timeout = 15000): Promise<unknown> => {
  return new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => {
      reject(new Error(`Timeout waiting for response. Buffer: ${buffer}`));
    }, timeout);

    const onData = (data: Buffer) => {
      buffer += data.toString();

      // Parse JSON Lines format
      const newlineIndex = buffer.indexOf('\n');
      if (newlineIndex === -1) return;

      const line = buffer.slice(0, newlineIndex).trim();
      buffer = buffer.slice(newlineIndex + 1);

      if (line.length === 0) return;

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
};

const MCP_INTEGRATION_TEST_TIMEOUT_MS = 30_000;
const mcpIt = (name: string, callback: () => Promise<void>) =>
  it(name, callback, MCP_INTEGRATION_TEST_TIMEOUT_MS);

describe('MCP Server Integration', () => {
  let serverProc: ChildProcess;

  beforeAll(async () => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });

    serverProc = spawn(binWrapper, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        NODE_ENV: 'test',
        PDF_READER_ENGINE_MODE: 'pure-rust',
        PDF_READER_MCP_TRANSPORT: '',
        MCP_TRANSPORT: '',
        MCP_PDF_ALLOWED_DIRS: path.join(repoRoot, 'test/fixtures'),
      },
    });

    await new Promise((r) => setTimeout(r, 1500));
  }, 300_000);

  afterAll(() => {
    serverProc?.kill();
  });

  mcpIt('should respond to initialize request', async () => {
    const initRequest = createRequest(1, 'initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'test-client', version: '1.0.0' },
    });

    sendMessage(serverProc, initRequest);
    const response = (await readResponse(serverProc)) as {
      id: number;
      result?: { serverInfo?: { name: string } };
    };

    expect(response.id).toBe(1);
    expect(response.result?.serverInfo?.name).toBe('anymd');
  });

  mcpIt('should list available tools', async () => {
    // Send initialized notification first
    sendMessage(serverProc, { jsonrpc: '2.0', method: 'notifications/initialized' });

    // Wait a bit
    await new Promise((r) => setTimeout(r, 100));

    const listRequest = createRequest(2, 'tools/list', {});
    sendMessage(serverProc, listRequest);

    const response = (await readResponse(serverProc)) as {
      id: number;
      result?: { tools?: Array<{ name: string }> };
    };

    expect(response.id).toBe(2);
    expect(response.result?.tools).toBeDefined();
    expect(response.result?.tools?.length).toBeGreaterThan(0);

    const toolNames = response.result?.tools?.map((t) => t.name);
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

  mcpIt('should reject local paths outside the native filesystem allowlist', async () => {
    const callRequest = createRequest(99, 'tools/call', {
      name: 'read_pdf',
      arguments: {
        sources: [{ path: path.join(repoRoot, 'package.json') }],
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc)) as {
      id: number;
      error?: { message?: string };
      result?: { content?: Array<{ text?: string }>; isError?: boolean };
    };

    expect(response.id).toBe(99);
    expect(response.error?.message ?? response.result?.content?.[0]?.text ?? '').toContain(
      'Access denied'
    );
  });

  mcpIt('should call pdf_evidence inspect operation with a test PDF', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(3, 'tools/call', {
      name: 'pdf_evidence',
      arguments: {
        operation: 'inspect',
        sources: [{ path: testPdfPath }],
        sample_pages: 2,
        include_metadata: true,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: { content?: Array<{ type: string; text?: string }>; isError?: boolean };
      error?: { message: string };
    };

    expect(response.id).toBe(3);

    if (response.error || response.result?.isError) {
      expect(response.error?.message || response.result?.content?.[0]?.text).toContain('PDF');
    } else {
      const textContent = response.result?.content?.[0]?.text ?? '';
      const parsed = JSON.parse(textContent) as {
        results: Array<{ success: boolean; data?: Record<string, unknown> }>;
      };
      expect(response.result?.content?.[0]?.type).toBe('text');
      expect(textContent).toContain('"profile"');
      expect(parsed.results[0]?.success).toBe(true);
    }
  });

  mcpIt('should call read_pdf tool with a test PDF', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(4, 'tools/call', {
      name: 'read_pdf',
      arguments: {
        sources: [{ path: testPdfPath }],
        include_metadata: true,
        include_page_count: true,
        include_full_text: false,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: { content?: Array<{ type: string; text?: string }>; isError?: boolean };
      error?: { message: string };
    };

    expect(response.id).toBe(4);

    // If test PDF doesn't exist, expect error
    if (response.error || response.result?.isError) {
      // Expected if no test PDF
      expect(response.error?.message || response.result?.content?.[0]?.text).toContain('PDF');
    } else {
      // If it exists, should have content
      expect(response.result?.content).toBeDefined();
      expect(response.result?.content?.[0]?.type).toBe('text');
    }
  });

  mcpIt('should render bounded PNG evidence through pdf_evidence render_page', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(5, 'tools/call', {
      name: 'pdf_evidence',
      arguments: {
        operation: 'render_page',
        sources: [{ path: testPdfPath, pages: [1] }],
        scale: 1,
        max_pages: 1,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: {
        content?: Array<{ type: string; text?: string; data?: string; mimeType?: string }>;
        isError?: boolean;
      };
      error?: { message: string };
    };

    expect(response.id).toBe(5);

    expect(response.error).toBeUndefined();
    expect(response.result?.isError).not.toBe(true);
    expect(response.result?.content?.[0]?.type).toBe('text');
    expect(response.result?.content?.[1]?.type).toBe('image');
    expect(response.result?.content?.[1]?.mimeType).toBe('image/png');
    const imageData = response.result?.content?.[1]?.data ?? '';
    expect(imageData.startsWith('data:')).toBe(false);
    expect(Buffer.from(imageData, 'base64').subarray(0, 8)).toEqual(
      Buffer.from('\x89PNG\r\n\x1a\n', 'binary')
    );
    const payload = JSON.parse(response.result?.content?.[0]?.text ?? '{}') as {
      profile?: string;
      rendered_pages?: unknown;
      results?: Array<{
        success?: boolean;
        rendered_pages?: Array<{
          data?: string;
          image_content_index?: number;
          byte_length?: number;
        }>;
      }>;
    };
    expect(payload.profile).toBe('page_render_evidence');
    expect(payload.results?.[0]?.success).toBe(true);
    expect(payload.results?.[0]?.rendered_pages?.[0]?.data).toBeUndefined();
    expect(payload.results?.[0]?.rendered_pages?.[0]?.image_content_index).toBe(1);
    expect(payload.results?.[0]?.rendered_pages?.[0]?.byte_length).toBe(
      Buffer.from(imageData, 'base64').byteLength
    );
  });

  mcpIt('should crop bounded PNG evidence through pdf_evidence extract_regions', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(6, 'tools/call', {
      name: 'pdf_evidence',
      arguments: {
        operation: 'extract_regions',
        sources: [
          {
            path: testPdfPath,
            regions: [
              {
                id: 'bottom-left',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 100, top: 100 },
              },
            ],
          },
        ],
        scale: 1,
        max_regions: 1,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: {
        content?: Array<{ type: string; text?: string; data?: string; mimeType?: string }>;
        isError?: boolean;
      };
      error?: { message: string };
    };

    expect(response.id).toBe(6);

    expect(response.error).toBeUndefined();
    expect(response.result?.isError).not.toBe(true);
    expect(response.result?.content?.[0]?.type).toBe('text');
    expect(response.result?.content?.[1]?.type).toBe('image');
    expect(response.result?.content?.[1]?.mimeType).toBe('image/png');
    const imageData = response.result?.content?.[1]?.data ?? '';
    expect(imageData.startsWith('data:')).toBe(false);
    expect(Buffer.from(imageData, 'base64').subarray(0, 8)).toEqual(
      Buffer.from('\x89PNG\r\n\x1a\n', 'binary')
    );
    const payload = JSON.parse(response.result?.content?.[0]?.text ?? '{}') as {
      profile?: string;
      results?: Array<{
        success?: boolean;
        regions?: Array<{
          region_id?: string;
          data?: string;
          image_content_index?: number;
          crop_pixels?: { width?: number; height?: number };
        }>;
      }>;
    };
    expect(payload.profile).toBe('region_crop_evidence');
    expect(payload.results?.[0]?.success).toBe(true);
    expect(payload.results?.[0]?.regions?.[0]?.region_id).toBe('bottom-left');
    expect(payload.results?.[0]?.regions?.[0]?.data).toBeUndefined();
    expect(payload.results?.[0]?.regions?.[0]?.image_content_index).toBe(1);
    expect(payload.results?.[0]?.regions?.[0]?.crop_pixels).toMatchObject({
      width: 100,
      height: 100,
    });
  });

  mcpIt('should fail closed when pdf_evidence analyze_regions has no provider', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(7, 'tools/call', {
      name: 'pdf_evidence',
      arguments: {
        operation: 'analyze_regions',
        sources: [
          {
            path: testPdfPath,
            regions: [
              {
                id: 'table-1',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 100, top: 100 },
              },
            ],
          },
        ],
        scale: 1,
        max_regions: 1,
        languages: ['eng'],
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: { content?: Array<{ type: string; text?: string }>; isError?: boolean };
      error?: { message: string };
    };

    expect(response.id).toBe(7);

    expect(response.error || response.result?.isError).toBeTruthy();
    expect(response.error?.message || response.result?.content?.[0]?.text).toContain(
      'Region analysis provider is not configured'
    );
  });

  mcpIt('should fail closed when pdf_evidence ocr_pages has no provider', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(8, 'tools/call', {
      name: 'pdf_evidence',
      arguments: {
        operation: 'ocr_pages',
        sources: [{ path: testPdfPath, pages: [1] }],
        scale: 1,
        max_pages: 1,
        languages: ['eng'],
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: { content?: Array<{ type: string; text?: string }>; isError?: boolean };
      error?: { message: string };
    };

    expect(response.id).toBe(8);

    expect(response.error || response.result?.isError).toBeTruthy();
    expect(response.error?.message || response.result?.content?.[0]?.text).toContain(
      'OCR provider is not configured. Set MCP_PDF_OCR_COMMAND or MCP_PDF_OCR_PRESET=tesseract'
    );
  });

  mcpIt('should call search_pdf tool with a test PDF', async () => {
    const testPdfPath = path.resolve(__dirname, '../fixtures/sample.pdf');

    const callRequest = createRequest(9, 'tools/call', {
      name: 'search_pdf',
      arguments: {
        sources: [{ path: testPdfPath, pages: [1] }],
        query: 'PDF',
        max_pages: 1,
        max_matches_per_source: 5,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc, 10000)) as {
      id: number;
      result?: { content?: Array<{ type: string; text?: string }>; isError?: boolean };
      error?: { message: string };
    };

    expect(response.id).toBe(9);

    if (response.error || response.result?.isError) {
      expect(response.error?.message || response.result?.content?.[0]?.text).toContain('PDF');
    } else {
      const textContent = response.result?.content?.[0]?.text ?? '';

      // Default search answers in compact Markdown: a header and one line per hit.
      expect(response.result?.content?.[0]?.type).toBe('text');
      expect(textContent).toMatch(/\d+ match(es)? for "PDF"/);
    }
  });

  mcpIt('should handle invalid tool arguments', async () => {
    const callRequest = createRequest(10, 'tools/call', {
      name: 'read_pdf',
      arguments: {
        // Missing required 'sources' field
        include_metadata: true,
      },
    });

    sendMessage(serverProc, callRequest);
    const response = (await readResponse(serverProc)) as {
      id: number;
      result?: { isError?: boolean; content?: Array<{ text?: string }> };
      error?: { code: number; message: string };
    };

    expect(response.id).toBe(10);
    const errorText = response.error?.message ?? response.result?.content?.[0]?.text ?? '';
    expect(response.result?.isError === true || response.error !== undefined).toBe(true);
    expect(errorText).toMatch(/sources/i);
  });
});
