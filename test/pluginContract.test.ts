import { describe, expect, it } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { productVersion, repoRoot } from './utils/cargoBinaries.js';

const workflowDir = path.join(repoRoot, '.github/workflows');

describe('public plugin supply-chain contract', () => {
  it('keeps the Claude plugin version in the release version authority', () => {
    const manifest = JSON.parse(
      readFileSync(path.join(repoRoot, '.claude-plugin/plugin.json'), 'utf8')
    ) as { name: string; version: string };
    expect(manifest.name).toBe('anymd');
    expect(manifest.version).toBe(productVersion);
    expect(manifest.version).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);

    const result = spawnSync(process.execPath, ['scripts/set-version.ts', '--check'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    expect(result.status).toBe(0);
  });

  it('pins every remote workflow and action reference to an immutable commit', () => {
    const files = readdirSync(workflowDir).filter((file) => /\.ya?ml$/.test(file));
    expect(files.length).toBeGreaterThan(0);
    for (const file of files) {
      const workflow = readFileSync(path.join(workflowDir, file), 'utf8');
      const references = [...workflow.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)];
      for (const reference of references) {
        const target = reference[1]?.replace(/^['"]|['"]$/g, '') ?? '';
        if (target.startsWith('./')) continue;
        expect(`${file}: ${target}`).toMatch(/@[a-f0-9]{40}$/);
      }
    }
  });
});
