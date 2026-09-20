import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { resolveRustCliBinary } from './engine/rust-hash.js';
import { loadPdfDocumentCore } from './pdf/loader.js';
import { cargoBinaryCandidates } from './utils/cargoTargetDir.js';
import { destroyLoadingTask } from './utils/pdfjs.js';

export type DoctorStatus = 'ok' | 'warn' | 'fail';

export interface DoctorCheck {
  id: string;
  status: DoctorStatus;
  message: string;
}

export interface DoctorReport {
  profile: 'pdf_reader_doctor';
  version: string;
  status: 'ready' | 'degraded' | 'unavailable';
  checks: DoctorCheck[];
}

const require = createRequire(import.meta.url);
const packageRoot = path.resolve(path.dirname(require.resolve('../package.json')));

const probeNode = (): DoctorCheck => {
  const version = process.versions.node;
  const major = Number.parseInt(version.split('.')[0] ?? '0', 10);
  if (major >= 22) {
    return {
      id: 'node',
      status: 'ok',
      message: `Node.js ${version} meets the >=22.13 requirement.`,
    };
  }

  return {
    id: 'node',
    status: 'warn',
    message: `Node.js ${version} is below the recommended >=22.13 runtime.`,
  };
};

const probePdfjsResources = (): DoctorCheck => {
  const pdfjsRoot = require.resolve('pdfjs-dist/package.json').replace('package.json', '');
  const requiredDirs = ['cmaps', 'standard_fonts', 'wasm', 'iccs'] as const;
  const missing = requiredDirs.filter((dir) => !existsSync(path.join(pdfjsRoot, dir)));

  if (missing.length === 0) {
    return {
      id: 'pdfjs_resources',
      status: 'ok',
      message: 'pdfjs-dist CMap, font, WASM, and ICC resources are present.',
    };
  }

  return {
    id: 'pdfjs_resources',
    status: 'fail',
    message: `pdfjs-dist install is incomplete; missing: ${missing.join(', ')}.`,
  };
};

const probeSamplePdf = async (): Promise<DoctorCheck> => {
  const samplePath = path.join(packageRoot, 'test/fixtures/sample.pdf');
  if (!existsSync(samplePath)) {
    return {
      id: 'sample_probe',
      status: 'fail',
      message: 'Checked-in sample.pdf fixture is missing from the install tree.',
    };
  }

  try {
    const document = await loadPdfDocumentCore({ path: samplePath }, samplePath);
    const pages = document.numPages;
    await destroyLoadingTask(document.loadingTask);
    if (pages >= 1) {
      return {
        id: 'sample_probe',
        status: 'ok',
        message: `Sample PDF loads successfully (${String(pages)} page(s)).`,
      };
    }

    return {
      id: 'sample_probe',
      status: 'fail',
      message: 'Sample PDF loaded but reported zero pages.',
    };
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      id: 'sample_probe',
      status: 'fail',
      message: `Sample PDF probe failed: ${message}`,
    };
  }
};

const probeRustMcpServer = (): DoctorCheck => {
  const candidates = [
    ...cargoBinaryCandidates(packageRoot, 'citra-mcp-server'),
    path.join(packageRoot, 'bin/native/citra-mcp-server'),
  ];
  const native = candidates.find((candidate) => existsSync(candidate));
  if (native) {
    return {
      id: 'rust_mcp_server',
      status: 'ok',
      message: `Rust MCP server binary is present at ${native}.`,
    };
  }

  return {
    id: 'rust_mcp_server',
    status: 'warn',
    message:
      'Rust MCP server binary is not built yet. Run `bun run build:rust` before wiring the MCP host to the published bin.',
  };
};

const probeRustHashCli = (): DoctorCheck => {
  const binary = resolveRustCliBinary();
  if (binary !== 'pdf-reader-cli' && existsSync(binary)) {
    return {
      id: 'rust_hash_cli',
      status: 'ok',
      message: `Rust hash CLI is available at ${binary}. Source hashing defaults to the native engine when built.`,
    };
  }

  return {
    id: 'rust_hash_cli',
    status: 'warn',
    message:
      'Rust hash CLI is not built. Run `cargo build --release` to enable the default native hash path.',
  };
};

const probeTesseract = (): DoctorCheck => {
  const result = spawnSync('tesseract', ['--version'], {
    encoding: 'utf8',
    timeout: 2_500,
  });

  if (result.status === 0) {
    const versionLine = (result.stdout || result.stderr || '').split('\n')[0]?.trim();
    return {
      id: 'tesseract',
      status: 'ok',
      message: versionLine ? `Tesseract available (${versionLine})` : 'Tesseract available',
    };
  }

  return {
    id: 'tesseract',
    status: 'warn',
    message:
      'Tesseract is not installed. OCR remains optional; text-layer extraction still works for selectable PDFs.',
  };
};

const aggregateStatus = (checks: DoctorCheck[]): DoctorReport['status'] => {
  if (checks.some((check) => check.status === 'fail')) {
    return 'unavailable';
  }
  if (checks.some((check) => check.status === 'warn')) {
    return 'degraded';
  }
  return 'ready';
};

export async function runDoctor(version: string): Promise<DoctorReport> {
  const checks = [
    probeNode(),
    probePdfjsResources(),
    probeRustHashCli(),
    probeRustMcpServer(),
    await probeSamplePdf(),
    probeTesseract(),
  ];

  return {
    profile: 'pdf_reader_doctor',
    version,
    status: aggregateStatus(checks),
    checks,
  };
}

export function formatDoctorReport(report: DoctorReport): string {
  return JSON.stringify(report, null, 2);
}
