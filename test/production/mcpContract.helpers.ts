/**
 * Shared helpers for production-path MCP contract tests.
 * Sole-Rust production path: dist/runtime-entry.js + platform native binary.
 * Historical TypeScript oracle remains buildable for tests, but is not the production entry.
 */
import { type ChildProcess, execSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { resolveCliPath, resolveServerPath } from '../utils/cargoBinaries.js';

export const repoRoot = path.resolve(import.meta.dirname, '../..');
export const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');
export const fixturesRoot = path.join(repoRoot, 'test/fixtures');

export type JsonRpcResponse = {
  jsonrpc?: string;
  id?: number | string;
  result?: {
    serverInfo?: { name?: string; version?: string; instructions?: string };
    tools?: Array<{ name: string; description?: string; inputSchema?: unknown }>;
    content?: Array<{ type?: string; text?: string; data?: string; mimeType?: string }>;
    isError?: boolean;
    structuredContent?: Record<string, unknown>;
  };
  error?: { code?: number; message?: string };
};

export const packageJson = JSON.parse(
  fs.readFileSync(path.join(repoRoot, 'package.json'), 'utf8')
) as {
  version: string;
  bin?: Record<string, string>;
  exports?: Record<string, string>;
  files?: string[];
};

const resolveStagedRustBinary = (): string | null => {
  const forced = process.env.CITRA_RUST_BIN;
  if (forced && fs.existsSync(forced)) return forced;
  const candidates = [
    resolveServerPath(),
    path.join(repoRoot, 'bin/native/citra-mcp-server'),
    path.join(repoRoot, 'bin/native/linux-x64-gnu/citra-mcp-server'),
    path.join(repoRoot, 'bin/native/linux-arm64-gnu/citra-mcp-server'),
    path.join(repoRoot, 'bin/native/darwin-arm64/citra-mcp-server'),
    path.join(repoRoot, 'bin/native/darwin-x64/citra-mcp-server'),
    path.join(repoRoot, 'bin/native/win32-x64-msvc/citra-mcp-server.exe'),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) return c;
  }
  return null;
};

export const ensureProductionArtifacts = (
  requestedMode?: 'pure-rust' | 'rust' | 'typescript-oracle'
) => {
  const mode = requestedMode ?? process.env.PDF_READER_ENGINE_MODE;
  if (mode === 'typescript-oracle') {
    execSync('bun run build:oracle-ts', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });
    if (!fs.existsSync(path.join(repoRoot, 'dist/index.js'))) {
      throw new Error('missing dist/index.js — TypeScript oracle path not built');
    }
    return;
  }
  // Sole-Rust production path. Reuse existing release binary when present to avoid
  // multi-suite rebuild thrash in CI (test:cov runs many production suites).
  if (!fs.existsSync(path.join(repoRoot, 'dist/runtime-entry.js'))) {
    execSync('bun run build:package', { cwd: repoRoot, stdio: 'pipe', timeout: 120_000 });
  }
  if (!resolveStagedRustBinary()) {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 420_000 });
  }
  if (!fs.existsSync(path.join(repoRoot, 'dist/runtime-entry.js'))) {
    throw new Error('missing dist/runtime-entry.js — sole-Rust launcher not built');
  }
  if (!resolveStagedRustBinary()) {
    throw new Error('missing staged/release pure-Rust MCP server binary');
  }
};

export const productionEnv = (overrides: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv => {
  const env = { ...process.env, ...overrides };
  env.NODE_ENV = env.NODE_ENV ?? 'test';
  env.MCP_TRANSPORT = env.MCP_TRANSPORT ?? 'stdio';
  const rustBin = resolveStagedRustBinary();
  if (rustBin && !env.CITRA_RUST_BIN) {
    env.CITRA_RUST_BIN = rustBin;
  }
  // Sole-Rust production does not use TS engine mode flags.
  if (!overrides.PDF_READER_ENGINE_MODE) {
    env.PDF_READER_ENGINE_MODE = undefined;
  }
  return env;
};

export const createRequest = (id: number, method: string, params?: unknown) => ({
  jsonrpc: '2.0' as const,
  id,
  method,
  params,
});

export const sendMessage = (proc: ChildProcess, message: object): void => {
  proc.stdin?.write(`${JSON.stringify(message)}\n`);
};

export const readResponse = (proc: ChildProcess, timeoutMs = 45_000): Promise<JsonRpcResponse> =>
  new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timeout waiting for MCP response. Buffer: ${buffer.slice(0, 2000)}`));
    }, timeoutMs);

    const onData = (data: Buffer) => {
      buffer += data.toString();
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        const trimmed = line.trim();
        if (
          !trimmed ||
          trimmed.startsWith('Content-Length') ||
          trimmed.startsWith('content-length')
        ) {
          continue;
        }
        if (trimmed === '') continue;
        try {
          const msg = JSON.parse(trimmed) as JsonRpcResponse;
          if (msg.id !== undefined || msg.result !== undefined || msg.error !== undefined) {
            cleanup();
            resolve(msg);
            return;
          }
        } catch {
          // ignore non-json
        }
      }
    };

    const onExit = (code: number | null) => {
      cleanup();
      reject(
        new Error(`MCP process exited early (code=${code}). Buffer: ${buffer.slice(0, 1000)}`)
      );
    };

    const cleanup = () => {
      clearTimeout(timer);
      proc.stdout?.off('data', onData);
      proc.off('exit', onExit);
    };

    proc.stdout?.on('data', onData);
    proc.on('exit', onExit);
  });

export const spawnProductionMcp = (envOverrides: NodeJS.ProcessEnv = {}): ChildProcess => {
  const mode = envOverrides.PDF_READER_ENGINE_MODE ?? process.env.PDF_READER_ENGINE_MODE;
  if (mode === 'typescript-oracle') {
    return spawn(process.execPath, [path.join(repoRoot, 'dist/index.js')], {
      cwd: repoRoot,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: productionEnv(envOverrides),
    });
  }
  // Sole-Rust production entry.
  return spawn(process.execPath, [path.join(repoRoot, 'dist/runtime-entry.js')], {
    cwd: repoRoot,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: productionEnv(envOverrides),
  });
};

export const initializeSession = async (
  proc: ChildProcess,
  clientName = 'production-contract'
): Promise<JsonRpcResponse> => {
  sendMessage(
    proc,
    createRequest(1, 'initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: clientName, version: '1.0.0' },
    })
  );
  const init = await readResponse(proc);
  sendMessage(proc, { jsonrpc: '2.0', method: 'notifications/initialized' });
  await new Promise((r) => setTimeout(r, 50));
  return init;
};

export const callTool = async (
  proc: ChildProcess,
  id: number,
  name: string,
  args: Record<string, unknown>,
  timeoutMs = 60_000
): Promise<JsonRpcResponse> => {
  sendMessage(
    proc,
    createRequest(id, 'tools/call', {
      name,
      arguments: args,
    })
  );
  return readResponse(proc, timeoutMs);
};

export const listTools = async (proc: ChildProcess, id = 2): Promise<JsonRpcResponse> => {
  sendMessage(proc, createRequest(id, 'tools/list', {}));
  return readResponse(proc);
};

export const parseToolPayload = (
  response: JsonRpcResponse
): { isError: boolean; text: string; structured?: Record<string, unknown> } => {
  if (response.error) {
    return { isError: true, text: response.error.message ?? JSON.stringify(response.error) };
  }
  const result = response.result;
  if (!result) {
    return { isError: true, text: 'missing result' };
  }
  if (result.isError) {
    const text = (result.content ?? []).map((part) => part.text ?? '').join('\n');
    return { isError: true, text: text || 'tool isError' };
  }
  if (result.structuredContent) {
    return {
      isError: false,
      text: JSON.stringify(result.structuredContent),
      structured: result.structuredContent,
    };
  }
  const text = (result.content ?? []).map((part) => part.text ?? '').join('\n');
  return { isError: false, text };
};
