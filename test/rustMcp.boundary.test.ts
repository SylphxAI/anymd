import { describe, expect, it } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { resolveServerPath } from './utils/cargoBinaries.js';

const repoRoot = path.resolve(import.meta.dirname, '..');
const rustServerBin = resolveServerPath();
const stagedRustBin = path.join(repoRoot, 'bin/native/anymd');
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');

describe('MCP transport boundary (pure-Rust)', () => {
  it('builds the rmcp stdio server binary for the production process path', () => {
    expect(existsSync(rustServerBin)).toBe(true);
    expect(existsSync(stagedRustBin)).toBe(true);
  });

  it('reports doctor diagnostics from the default Rust MCP entrypoint', () => {
    const result = spawnSync(rustServerBin, ['doctor'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });

    const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
    expect(output).toContain('(native Rust)');
    expect(output).toContain('tesseract');
  });

  it('converts a file to Markdown on stdout in CLI mode', () => {
    const result = spawnSync(rustServerBin, [samplePdf], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain('<!-- page 1 -->');
  });
});
