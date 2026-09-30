import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { resolveServerPath } from '../utils/cargoBinaries.js';
import {
  callTool,
  ensureProductionArtifacts,
  fixturesRoot,
  initializeSession,
  parseToolPayload,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

type OutlineNode = {
  id: string;
  title: string;
  level: number;
  from: number;
  to: number;
  start: number;
  end: number;
  children: number;
  path: string;
};
type Outline = { nodes: OutlineNode[]; format: string };

const cli = (...args: string[]) => {
  const result = spawnSync(resolveServerPath(), args, {
    encoding: 'utf8',
    timeout: 60_000,
  });
  expect(result.error).toBeUndefined();
  expect(result.status).toBe(0);
  return result.stdout;
};

describe('outline navigation public contract', () => {
  let proc: ChildProcess;
  let temporary: string;
  let id = 200;
  beforeAll(async () => {
    ensureProductionArtifacts();
    temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'anymd-outline-'));
    fs.writeFileSync(
      path.join(temporary, 'notes.md'),
      '# Report\n\nIntro.\n\n## Revenue\n\nIncome rose.\n\n### Costs\n\nCosts fell.\n\n## Next\n\nDo not include this.\n'
    );
    fs.writeFileSync(
      path.join(temporary, 'page.html'),
      '<html><body><h1>Report</h1><h2>Revenue</h2><p>Income rose.</p><h2>Next</h2><p>End.</p></body></html>'
    );
    proc = spawnProductionMcp();
    await initializeSession(proc, 'outline-contract');
  }, 420_000);
  afterAll(() => {
    proc?.kill('SIGTERM');
    if (temporary) fs.rmSync(temporary, { recursive: true, force: true });
  });

  test('each format returns stable ids, bounded ranges, and readable nodes', async () => {
    for (const source of [
      path.join(fixturesRoot, 'sample.pdf'),
      path.join(fixturesRoot, 'differential/v3014-behavior-v1.pdf'),
      path.join(fixturesRoot, 'alt-text.docx'),
      path.join(fixturesRoot, 'slides.pptx'),
      path.join(fixturesRoot, 'field-notes.epub'),
      path.join(temporary, 'notes.md'),
      path.join(temporary, 'page.html'),
    ]) {
      const response = parseToolPayload(await callTool(proc, ++id, 'outline', { source }));
      expect(response.isError).toBe(false);
      const outline = JSON.parse(response.text) as Outline;
      expect(outline.nodes.length).toBeGreaterThan(1);
      expect(JSON.parse(cli('outline', source, '--format', 'json'))).toEqual(outline);
      expect(cli('outline', source)).toContain(outline.nodes[1].id);
      const body = Buffer.from(cli(source, '--images', 'none', '--no-ocr'));
      // The canonical outline body ends with two newlines, while the CLI trims output.
      const canonical = Buffer.concat([
        Buffer.from(body.toString().trimEnd()),
        Buffer.from('\n\n'),
      ]);
      for (const node of outline.nodes) {
        expect(node.start).toBeLessThanOrEqual(node.end);
        expect(node.end).toBeLessThanOrEqual(canonical.length);
        expect(node.from).toBeLessThanOrEqual(node.to);
        const result = parseToolPayload(
          await callTool(proc, ++id, 'read', {
            source,
            node: node.id,
            images: 'none',
            ocr: false,
          })
        );
        expect(result.isError).toBe(false);
        expect(result.text.trim().length).toBeGreaterThan(0);
      }
    }
  }, 240_000);

  test('read clips to a heading subtree and search points to its deepest node', async () => {
    const source = path.join(temporary, 'notes.md');
    const outline = JSON.parse(cli('outline', source, '--json')) as Outline;
    const revenue = outline.nodes.find((node) => node.title === 'Revenue');
    const costs = outline.nodes.find((node) => node.title === 'Costs');
    expect(revenue).toBeDefined();
    expect(costs).toBeDefined();
    expect(revenue?.children).toBe(1);
    const text = cli(source, '--node', revenue?.id ?? '');
    expect(text).toContain('Income rose.');
    expect(text).toContain('Costs fell.');
    expect(text).not.toContain('Do not include this.');
    expect(text).not.toContain('Intro.');
    for (const mode of ['literal', 'ranked']) {
      const hit = parseToolPayload(
        await callTool(proc, ++id, 'search', {
          query: 'Costs',
          sources: [source],
          mode,
        })
      );
      expect(hit.isError).toBe(false);
      expect(hit.text).toContain(`node ${costs?.id}`);
      expect(hit.text).toContain('Report > Revenue > Costs');
    }
    const invalid = parseToolPayload(
      await callTool(proc, ++id, 'read', { source, node: 'missing' })
    );
    expect(invalid.isError).toBe(true);
    expect(invalid.text).toContain('Unknown node');
  }, 60_000);

  test('ranked Unicode hits retain their source node after case expansion', async () => {
    const source = path.join(temporary, 'unicode.md');
    fs.writeFileSync(
      source,
      '# Report\n\n## Revenue\n\nİncome growth increased.\n\n## Next\n\nEnd.\n'
    );
    const outline = JSON.parse(cli('outline', source, '--json')) as Outline;
    const node = outline.nodes.find((item) => item.title === 'Revenue');
    const hit = parseToolPayload(
      await callTool(proc, ++id, 'search', { query: 'growth', sources: [source], mode: 'ranked' })
    );
    expect(hit.isError).toBe(false);
    expect(hit.text).toContain(`node ${node?.id}`);
    expect(hit.text).toContain('Report > Revenue');
  });

  test('node paging uses original byte offsets and never reaches the next sibling', async () => {
    const source = path.join(temporary, 'long.md');
    const lines = Array.from({ length: 400 }, (_, n) => `Revenue line ${n}: Income grows.`).join(
      '\n'
    );
    fs.writeFileSync(source, `# Report\n\n## Revenue\n\n${lines}\n\n## Next\n\nOutside.\n`);
    const outline = JSON.parse(cli('outline', source, '--json')) as Outline;
    const node = outline.nodes.find((item) => item.title === 'Revenue')?.id ?? '';
    let cursor: string | undefined;
    let accumulated = '';
    for (let count = 0; count < 30; count++) {
      const result = parseToolPayload(
        await callTool(proc, ++id, 'read', {
          source,
          node,
          max_tokens: 500,
          cursor,
          images: 'none',
        })
      );
      expect(result.isError).toBe(false);
      expect(result.text).not.toContain('Outside.');
      accumulated += result.text;
      const next = result.text.match(/Continue with cursor: "([^"]+)"/);
      if (!next) break;
      expect(next[1]).not.toBe(cursor);
      cursor = next[1];
    }
    expect(accumulated).toContain('Revenue line 0:');
    expect(accumulated).toContain('Revenue line 399:');
    for (let n = 0; n < 400; n++) {
      expect(accumulated.split(`Revenue line ${n}:`).length - 1).toBe(1);
    }
  }, 120_000);
});
