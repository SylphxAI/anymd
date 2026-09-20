/**
 * Thin pure-Rust process client for @sylphx/citra.
 *
 * Does not process PDFs in TypeScript. Spawns the platform native binary only.
 * Production package default is dist/runtime-entry.js (sole-Rust; ADR-0006).
 *
 * Import:
 *   import { createPureRustClient, resolvePureRustServerBinary } from '@sylphx/citra/pure-rust'
 */
import { type ChildProcessWithoutNullStreams, spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  NATIVE_PLATFORM_PACKAGES,
  type NativePlatformId,
  nativeBinaryRelativePath,
  resolveNativePlatformId,
} from './native/platform-package-map.js';
import { cargoBinaryCandidates } from './utils/cargoTargetDir.js';

export type { NativePlatformId };

export const PURE_RUST_EXPORT = {
  status: 'sole-rust-production' as const,
  dropInFor3014: true,
  publishFreeze: false,
  engineMode: 'pure-rust' as const,
  defaultPackageExport: './dist/runtime-entry.js',
  pureRustExport: './dist/pure-rust.js',
};

const require = createRequire(import.meta.url);

const packageRootFromThisModule = (): string => {
  const here = dirname(fileURLToPath(import.meta.url));
  return join(here, '..');
};

const pushPlatformCandidates = (
  candidates: string[],
  packageRoot: string,
  platformId: NativePlatformId
) => {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  candidates.push(
    join(packageRoot, nativeBinaryRelativePath(platformId)),
    join(packageRoot, meta.packageDir, 'bin', meta.binaryName),
    join(packageRoot, 'node_modules', meta.npmName, 'bin', meta.binaryName)
  );
  try {
    const optionalPkgJson = require.resolve(`${meta.npmName}/package.json`, {
      paths: [packageRoot],
    });
    candidates.push(join(dirname(optionalPkgJson), 'bin', meta.binaryName));
  } catch {
    // optional package not installed
  }
};

const pushFallbackCandidates = (candidates: string[], packageRoot: string) => {
  candidates.push(
    ...cargoBinaryCandidates(packageRoot, 'citra-mcp-server'),
    join(packageRoot, 'bin/native/citra-mcp-server'),
    join(packageRoot, 'bin/native/citra-mcp-server.exe')
  );
};

export const resolvePureRustServerBinary = (options?: {
  packageRoot?: string;
  platformId?: NativePlatformId | null;
  env?: NodeJS.ProcessEnv;
}): string | null => {
  const env = options?.env ?? process.env;
  const explicit = env['CITRA_RUST_BIN']?.trim();
  if (explicit && existsSync(explicit)) return explicit;

  const packageRoot = options?.packageRoot ?? packageRootFromThisModule();
  const platformId =
    options && Object.hasOwn(options, 'platformId')
      ? options.platformId
      : resolveNativePlatformId();
  const candidates: string[] = [];
  if (platformId) pushPlatformCandidates(candidates, packageRoot, platformId);
  pushFallbackCandidates(candidates, packageRoot);
  return candidates.find((candidate) => existsSync(candidate)) ?? null;
};

type JsonRpcResponse = {
  id?: number | string;
  result?: unknown;
  error?: { message?: string; code?: number; data?: unknown };
};

export type PureRustToolName = 'read_pdf' | 'search_pdf' | 'pdf_evidence';

export type PureRustClientOptions = {
  binaryPath?: string;
  packageRoot?: string;
  timeoutMs?: number;
  env?: NodeJS.ProcessEnv;
};

export type PureRustCallResult = {
  raw: JsonRpcResponse;
  payload: Record<string, unknown>;
  isError: boolean;
};

const textContentPayload = (result: Record<string, unknown>): Record<string, unknown> | null => {
  const content = result['content'];
  if (!Array.isArray(content)) return null;
  const textPart = content.find(
    (entry) => entry && typeof entry === 'object' && (entry as { type?: string }).type === 'text'
  ) as { text?: string } | undefined;
  if (!textPart?.text) return null;
  return JSON.parse(textPart.text) as Record<string, unknown>;
};

const publicPayload = (response: JsonRpcResponse): Record<string, unknown> => {
  const result = response.result as Record<string, unknown> | undefined;
  if (!result) {
    throw new Error(`missing result: ${JSON.stringify(response).slice(0, 500)}`);
  }
  const fromText = textContentPayload(result);
  if (fromText) return fromText;
  const structured = result['structuredContent'];
  if (structured && typeof structured === 'object') {
    return structured as Record<string, unknown>;
  }
  return result;
};

const consumeJsonLine = (
  line: string,
  pending: Map<number, (value: JsonRpcResponse) => void>
): void => {
  try {
    const response = JSON.parse(line) as JsonRpcResponse;
    const resolver = pending.get(Number(response.id));
    if (!resolver) return;
    pending.delete(Number(response.id));
    resolver(response);
  } catch {
    // ignore non-JSON noise
  }
};

const drainStdout = (
  buffer: string,
  pending: Map<number, (value: JsonRpcResponse) => void>
): string => {
  let rest = buffer;
  while (rest.includes('\n')) {
    const index = rest.indexOf('\n');
    const line = rest.slice(0, index).trim();
    rest = rest.slice(index + 1);
    if (line) consumeJsonLine(line, pending);
  }
  return rest;
};

export class PureRustClient {
  readonly binaryPath: string;
  readonly timeoutMs: number;
  readonly env: NodeJS.ProcessEnv;

  constructor(options: PureRustClientOptions = {}) {
    const resolveOptions: { packageRoot?: string; env?: NodeJS.ProcessEnv } = {};
    if (options.packageRoot !== undefined) resolveOptions.packageRoot = options.packageRoot;
    if (options.env !== undefined) resolveOptions.env = options.env;
    const binaryPath = options.binaryPath ?? resolvePureRustServerBinary(resolveOptions);
    if (!binaryPath) {
      throw new Error(
        'Pure-Rust MCP server binary not found. Build/stage with `bun run build:rust` or set CITRA_RUST_BIN.'
      );
    }
    this.binaryPath = binaryPath;
    this.timeoutMs = options.timeoutMs ?? 45_000;
    this.env = {
      ...process.env,
      ...options.env,
      MCP_TRANSPORT: 'stdio',
      PDF_READER_ENGINE_MODE: 'pure-rust',
    };
  }

  private openChild(): {
    child: ChildProcessWithoutNullStreams;
    request: (
      id: number,
      method: string,
      params: Record<string, unknown>
    ) => Promise<JsonRpcResponse>;
    close: () => void;
  } {
    const child = spawn(this.binaryPath, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: this.env,
    }) as ChildProcessWithoutNullStreams;
    let buffer = '';
    let stderr = '';
    const pending = new Map<number, (value: JsonRpcResponse) => void>();
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
    });
    child.stdout.on('data', (chunk) => {
      buffer = drainStdout(buffer + chunk.toString(), pending);
    });
    const request = (id: number, method: string, params: Record<string, unknown>) =>
      new Promise<JsonRpcResponse>((resolve, reject) => {
        const timer = setTimeout(() => {
          reject(new Error(`timeout ${method}: ${stderr.slice(-2000)}`));
        }, this.timeoutMs);
        pending.set(id, (value) => {
          clearTimeout(timer);
          resolve(value);
        });
        child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
      });
    return {
      child,
      request,
      close: () => {
        child.kill('SIGTERM');
      },
    };
  }

  async callTool(
    name: PureRustToolName,
    args: Record<string, unknown>
  ): Promise<PureRustCallResult> {
    const session = this.openChild();
    try {
      await session.request(1, 'initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'pdf-reader-mcp-pure-rust-library', version: '1' },
      });
      session.child.stdin.write(
        `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
      );
      const raw = await session.request(2, 'tools/call', { name, arguments: args });
      if (raw.error) {
        throw new Error(raw.error.message ?? JSON.stringify(raw.error));
      }
      const result = (raw.result as Record<string, unknown> | undefined) ?? {};
      return {
        raw,
        payload: publicPayload(raw),
        isError: result['isError'] === true,
      };
    } finally {
      session.close();
    }
  }

  readPdf(args: Record<string, unknown>): Promise<PureRustCallResult> {
    return this.callTool('read_pdf', args);
  }

  searchPdf(args: Record<string, unknown>): Promise<PureRustCallResult> {
    return this.callTool('search_pdf', args);
  }

  pdfEvidence(args: Record<string, unknown>): Promise<PureRustCallResult> {
    return this.callTool('pdf_evidence', args);
  }
}

export const createPureRustClient = (options?: PureRustClientOptions): PureRustClient =>
  new PureRustClient(options);

export const getPureRustExportContract = () => ({
  ...PURE_RUST_EXPORT,
  resolvedBinary: resolvePureRustServerBinary(),
  platformId: resolveNativePlatformId(),
});
