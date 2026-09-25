/**
 * Shared helpers for production-path MCP contract tests. They run the
 * cargo-built anymd binary, the same binary the npm launcher starts.
 */
import { type ChildProcess, execSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { repoRoot, resolveServerPath } from '../utils/cargoBinaries.js';

export { repoRoot };
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

export const ensureProductionArtifacts = () => {
  // Reuse an existing build so the suites do not rebuild in turn.
  if (!fs.existsSync(resolveServerPath())) {
    execSync('cargo build --release -p anymd', { cwd: repoRoot, stdio: 'pipe', timeout: 420_000 });
  }
  if (!fs.existsSync(resolveServerPath())) {
    throw new Error(`missing anymd binary at ${resolveServerPath()}`);
  }
};

export const productionEnv = (overrides: NodeJS.ProcessEnv = {}): NodeJS.ProcessEnv => {
  const env = { ...process.env, ...overrides };
  env.NODE_ENV = env.NODE_ENV ?? 'test';
  env.MCP_TRANSPORT = env.MCP_TRANSPORT ?? 'stdio';
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
  return spawn(resolveServerPath(), [], {
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
