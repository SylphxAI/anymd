import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { type ChildProcess, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolveServerPath } from '../utils/cargoBinaries.js';
import {
  callTool,
  ensureProductionArtifacts,
  initializeSession,
  listTools,
  parseToolPayload,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

const encodings = ['utf8', 'utf8-bom', 'utf16le', 'utf16be'] as const;
type Encoding = (typeof encodings)[number];

function encode(text: string, encoding: Encoding): Buffer {
  if (encoding === 'utf8') return Buffer.from(text);
  if (encoding === 'utf8-bom') return Buffer.from(`\uFEFF${text}`);
  const bytes = Buffer.from(`\uFEFF${text}`, 'utf16le');
  return encoding === 'utf16be' ? bytes.swap16() : bytes;
}

function table(text: string): string {
  return text
    .split('\n')
    .filter((line) => line.startsWith('|'))
    .join('\n');
}

describe('delimited text on the production CLI and MCP read surfaces', () => {
  let proc: ChildProcess;
  let directory: string;
  let id = 10;

  beforeAll(async () => {
    ensureProductionArtifacts();
    directory = mkdtempSync(join(tmpdir(), 'anymd-delimited-'));
    proc = spawnProductionMcp();
    const initialized = await initializeSession(proc, 'delimited-text-contract');
    expect(initialized.error).toBeUndefined();
    const listed = await listTools(proc);
    expect(listed.result?.tools?.some((tool) => tool.name === 'read')).toBe(true);
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
    if (directory) rmSync(directory, { recursive: true, force: true });
  });

  for (const [extension, delimiter] of [
    ['csv', ','],
    ['tsv', '\t'],
  ] as const) {
    test(`${extension}: same cells, order and rows across four encodings`, async () => {
      const fixtures = [
        {
          text: `name${delimiter}city\r\nAlice${delimiter}Hong Kong\r\n陳小明${delimiter}香港\r\n`,
          expected: '|name|city|\n|-|-|\n|Alice|Hong Kong|\n|陳小明|香港|',
        },
        {
          text: `name${delimiter}note\r\n"陳小明😀${delimiter}A"${delimiter}"says ""hi""\r\n香港|港"\r\nAlice${delimiter}short\r\n`,
          expected: `|name|note|\n|-|-|\n|陳小明😀${delimiter}A|says "hi" 香港\\|港|\n|Alice|short|`,
        },
      ];
      for (const [index, fixture] of fixtures.entries()) {
        for (const encoding of encodings) {
          const source = join(directory, `${index}-${encoding}.${extension}`);
          writeFileSync(source, encode(fixture.text, encoding));
          const cli = spawnSync(resolveServerPath(), [source], { encoding: 'utf8' });
          expect(cli.status).toBe(0);
          expect(table(cli.stdout)).toBe(fixture.expected);
          expect(cli.stdout).not.toMatch(/[\0\uFFFD]/u);
          const mcp = parseToolPayload(await callTool(proc, id++, 'read', { source }));
          expect(mcp.isError).toBe(false);
          expect(table(mcp.text)).toBe(fixture.expected);
          expect(mcp.text).not.toMatch(/[\0\uFFFD]/u);
        }
      }
    });

    test(`${extension}: malformed UTF-16 fails and the same MCP session recovers`, async () => {
      for (const encoding of ['utf16le', 'utf16be'] as const) {
        for (const broken of [
          Buffer.concat([encode('name\r\n', encoding), Buffer.from([0x41])]),
          encode('name\r\n\uD800', encoding),
          encode('name\r\n\uDC00', encoding),
        ]) {
          const source = join(directory, `broken-${id}.${extension}`);
          writeFileSync(source, broken);
          const cli = spawnSync(resolveServerPath(), [source], { encoding: 'utf8' });
          expect(cli.status).not.toBe(0);
          expect(`${cli.stdout}${cli.stderr}`).toContain('re-export');
          const mcp = parseToolPayload(await callTool(proc, id++, 'read', { source }));
          expect(mcp.isError).toBe(true);
          expect(mcp.text).toContain('re-export');

          const valid = join(directory, `recovered-${id}.${extension}`);
          writeFileSync(
            valid,
            encode(`name${delimiter}city\r\n陳小明${delimiter}香港\r\n`, encoding)
          );
          const recovered = parseToolPayload(await callTool(proc, id++, 'read', { source: valid }));
          expect(recovered.isError).toBe(false);
          expect(table(recovered.text)).toBe('|name|city|\n|-|-|\n|陳小明|香港|');
        }
      }
    });
  }
});
