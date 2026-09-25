import { describe, expect, it } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { productVersion, repoRoot, resolveServerPath } from './utils/cargoBinaries.js';

const rustServerBin = resolveServerPath();
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');

describe('MCP transport boundary (pure-Rust)', () => {
  it('builds the rmcp stdio server binary for the production process path', () => {
    expect(existsSync(rustServerBin)).toBe(true);
  });

  it('prints the product version, which the release smoke test runs', () => {
    const result = spawnSync(rustServerBin, ['version'], { encoding: 'utf8' });
    expect(result.status).toBe(0);
    expect(result.stdout.trim()).toBe(`anymd ${productVersion}`);
  });

  it('previews MCP client registration without writing', () => {
    const result = spawnSync(rustServerBin, ['setup', '--dry-run', '--client=cursor'], {
      encoding: 'utf8',
    });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain('anymd setup (dry run)');
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
