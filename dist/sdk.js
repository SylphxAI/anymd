// src/pure-rust.ts
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// src/native/platform-package-map.ts
var NATIVE_PLATFORM_PACKAGES = {
  "darwin-arm64": {
    npmName: "@sylphx/citra-darwin-arm64",
    packageDir: "packages/citra-darwin-arm64",
    binaryName: "citra-mcp-server",
    os: "darwin",
    cpu: "arm64"
  },
  "darwin-x64": {
    npmName: "@sylphx/citra-darwin-x64",
    packageDir: "packages/citra-darwin-x64",
    binaryName: "citra-mcp-server",
    os: "darwin",
    cpu: "x64"
  },
  "linux-arm64-gnu": {
    npmName: "@sylphx/citra-linux-arm64-gnu",
    packageDir: "packages/citra-linux-arm64-gnu",
    binaryName: "citra-mcp-server",
    os: "linux",
    cpu: "arm64"
  },
  "linux-x64-gnu": {
    npmName: "@sylphx/citra-linux-x64-gnu",
    packageDir: "packages/citra-linux-x64-gnu",
    binaryName: "citra-mcp-server",
    os: "linux",
    cpu: "x64"
  },
  "win32-x64-msvc": {
    npmName: "@sylphx/citra-win32-x64-msvc",
    packageDir: "packages/citra-win32-x64-msvc",
    binaryName: "citra-mcp-server.exe",
    os: "win32",
    cpu: "x64"
  }
};
var resolveNativePlatformId = (platform = process.platform, arch = process.arch) => {
  if (platform === "darwin" && arch === "arm64")
    return "darwin-arm64";
  if (platform === "darwin" && arch === "x64")
    return "darwin-x64";
  if (platform === "linux" && arch === "arm64")
    return "linux-arm64-gnu";
  if (platform === "linux" && arch === "x64")
    return "linux-x64-gnu";
  if (platform === "win32" && arch === "x64")
    return "win32-x64-msvc";
  return null;
};
var nativeBinaryRelativePath = (platformId) => {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  return `bin/native/${platformId}/${meta.binaryName}`;
};
if (false) {}

// src/pure-rust.ts
var require2 = createRequire(import.meta.url);
var packageRootFromThisModule = () => {
  const here = dirname(fileURLToPath(import.meta.url));
  return join(here, "..");
};
var pushPlatformCandidates = (candidates, packageRoot, platformId) => {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  candidates.push(join(packageRoot, nativeBinaryRelativePath(platformId)), join(packageRoot, meta.packageDir, "bin", meta.binaryName), join(packageRoot, "node_modules", meta.npmName, "bin", meta.binaryName));
  try {
    const optionalPkgJson = require2.resolve(`${meta.npmName}/package.json`, {
      paths: [packageRoot]
    });
    candidates.push(join(dirname(optionalPkgJson), "bin", meta.binaryName));
  } catch {}
};
var pushFallbackCandidates = (candidates, packageRoot) => {
  const cargoTargetDir = process.env["CARGO_TARGET_DIR"]?.trim();
  const targetDir = cargoTargetDir ? resolve(cargoTargetDir) : join(packageRoot, "target");
  candidates.push(join(packageRoot, "bin/native/citra-mcp-server"), join(packageRoot, "bin/native/citra-mcp-server.exe"), join(targetDir, "release/citra-mcp-server"), join(targetDir, "release/citra-mcp-server.exe"), join(targetDir, "debug/citra-mcp-server"), join(targetDir, "debug/citra-mcp-server.exe"), join(packageRoot, "target/release/citra-mcp-server"), join(packageRoot, "target/release/citra-mcp-server.exe"), join(packageRoot, "target/debug/citra-mcp-server"), join(packageRoot, "target/debug/citra-mcp-server.exe"));
};
var resolvePureRustServerBinary = (options) => {
  const env = options?.env ?? process.env;
  const explicit = env["CITRA_RUST_BIN"]?.trim();
  if (explicit && existsSync(explicit))
    return explicit;
  const packageRoot = options?.packageRoot ?? packageRootFromThisModule();
  const platformId = options && Object.hasOwn(options, "platformId") ? options.platformId : resolveNativePlatformId();
  const candidates = [];
  if (platformId)
    pushPlatformCandidates(candidates, packageRoot, platformId);
  pushFallbackCandidates(candidates, packageRoot);
  return candidates.find((candidate) => existsSync(candidate)) ?? null;
};
var textContentPayload = (result) => {
  const content = result["content"];
  if (!Array.isArray(content))
    return null;
  const textPart = content.find((entry) => entry && typeof entry === "object" && entry.type === "text");
  if (!textPart?.text)
    return null;
  return JSON.parse(textPart.text);
};
var publicPayload = (response) => {
  const result = response.result;
  if (!result) {
    throw new Error(`missing result: ${JSON.stringify(response).slice(0, 500)}`);
  }
  const fromText = textContentPayload(result);
  if (fromText)
    return fromText;
  const structured = result["structuredContent"];
  if (structured && typeof structured === "object") {
    return structured;
  }
  return result;
};
var consumeJsonLine = (line, pending) => {
  try {
    const response = JSON.parse(line);
    const resolver = pending.get(Number(response.id));
    if (!resolver)
      return;
    pending.delete(Number(response.id));
    resolver(response);
  } catch {}
};
var drainStdout = (buffer, pending) => {
  let rest = buffer;
  while (rest.includes(`
`)) {
    const index = rest.indexOf(`
`);
    const line = rest.slice(0, index).trim();
    rest = rest.slice(index + 1);
    if (line)
      consumeJsonLine(line, pending);
  }
  return rest;
};

class PureRustClient {
  binaryPath;
  timeoutMs;
  env;
  constructor(options = {}) {
    const resolveOptions = {};
    if (options.packageRoot !== undefined)
      resolveOptions.packageRoot = options.packageRoot;
    if (options.env !== undefined)
      resolveOptions.env = options.env;
    const binaryPath = options.binaryPath ?? resolvePureRustServerBinary(resolveOptions);
    if (!binaryPath) {
      throw new Error("Pure-Rust MCP server binary not found. Build/stage with `bun run build:rust` or set CITRA_RUST_BIN.");
    }
    this.binaryPath = binaryPath;
    this.timeoutMs = options.timeoutMs ?? 45000;
    this.env = {
      ...process.env,
      ...options.env,
      MCP_TRANSPORT: "stdio",
      PDF_READER_ENGINE_MODE: "pure-rust"
    };
  }
  openChild() {
    const child = spawn(this.binaryPath, [], {
      stdio: ["pipe", "pipe", "pipe"],
      env: this.env
    });
    let buffer = "";
    let stderr = "";
    const pending = new Map;
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.stdout.on("data", (chunk) => {
      buffer = drainStdout(buffer + chunk.toString(), pending);
    });
    const request = (id, method, params) => new Promise((resolve2, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`timeout ${method}: ${stderr.slice(-2000)}`));
      }, this.timeoutMs);
      pending.set(id, (value) => {
        clearTimeout(timer);
        resolve2(value);
      });
      child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}
`);
    });
    return {
      child,
      request,
      close: () => {
        child.kill("SIGTERM");
      }
    };
  }
  async callTool(name, args) {
    const session = this.openChild();
    try {
      await session.request(1, "initialize", {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "pdf-reader-mcp-pure-rust-library", version: "1" }
      });
      session.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}
`);
      const raw = await session.request(2, "tools/call", { name, arguments: args });
      if (raw.error) {
        throw new Error(raw.error.message ?? JSON.stringify(raw.error));
      }
      const result = raw.result ?? {};
      return {
        raw,
        payload: publicPayload(raw),
        isError: result["isError"] === true
      };
    } finally {
      session.close();
    }
  }
  readPdf(args) {
    return this.callTool("read_pdf", args);
  }
  searchPdf(args) {
    return this.callTool("search_pdf", args);
  }
  pdfEvidence(args) {
    return this.callTool("pdf_evidence", args);
  }
}
var createPureRustClient = (options) => new PureRustClient(options);

// src/sdk.ts
class Citra {
  client;
  constructor(options = {}) {
    this.client = createPureRustClient(options);
  }
  static create(options) {
    return new Citra(options);
  }
  read(input) {
    return this.client.readPdf(input);
  }
  search(input) {
    return this.client.searchPdf(input);
  }
  evidence(input) {
    return this.client.pdfEvidence(input);
  }
  call(tool, args) {
    return this.client.callTool(tool, args);
  }
}
var sdk_default = Citra;
export {
  Citra,
  PureRustClient,
  createPureRustClient,
  sdk_default as default,
  resolvePureRustServerBinary
};
