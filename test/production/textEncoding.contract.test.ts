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
  parseToolPayload,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

const encodings = ['utf8', 'utf8-bom', 'utf16le', 'utf16be'] as const;
type Encoding = (typeof encodings)[number];
const sentence = 'Alice in Hong Kong; 陳小明在香港; amber-cobalt-729. 😀𠮷';
const text = `# Records\r\n\r\n${sentence}\r\nSecond paragraph.\r\n`;
const html = `<!doctype html>\r\n<html><head><title>Records 陳小明😀</title></head><body><main><h1>Records</h1><p>${sentence}</p><p>Second paragraph.</p><table><tr><th>Name</th><th>City</th></tr><tr><td>陳小明😀</td><td>香港</td></tr></table><script>unsafe-marker</script><a href="javascript:alert(1)">safe label</a></main></body></html>\r\n`;

function encode(value: string, encoding: Encoding): Buffer {
  if (encoding === 'utf8') return Buffer.from(value);
  if (encoding === 'utf8-bom') return Buffer.from(`\uFEFF${value}`);
  const bytes = Buffer.from(`\uFEFF${value}`, 'utf16le');
  return encoding === 'utf16be' ? bytes.swap16() : bytes;
}

function withoutSource(value: string): string {
  return value.replace(/^source:.*$/m, 'source: FIXTURE');
}

describe('BOM documents on the real CLI and MCP read surfaces', () => {
  let proc: ChildProcess;
  let directory: string;
  let id = 10;

  beforeAll(async () => {
    ensureProductionArtifacts();
    directory = mkdtempSync(join(tmpdir(), 'anymd-text-encoding-'));
    proc = spawnProductionMcp();
    expect((await initializeSession(proc, 'text-encoding-contract')).error).toBeUndefined();
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
    if (directory) rmSync(directory, { recursive: true, force: true });
  });

  for (const extension of ['html', 'htm', 'md', 'txt']) {
    const isHtml = extension === 'html' || extension === 'htm';
    test(`${extension}: same structure, Unicode and provenance across four encodings`, async () => {
      let cliControl: string | undefined;
      let mcpControl: string | undefined;
      for (const encoding of encodings) {
        const source = join(directory, `${encoding}.${extension}`);
        writeFileSync(source, encode(isHtml ? html : text, encoding));
        const cli = spawnSync(resolveServerPath(), [source, '--front-matter'], {
          encoding: 'utf8',
        });
        expect(cli.status).toBe(0);
        const mcp = parseToolPayload(await callTool(proc, id++, 'read', { source }));
        expect(mcp.isError).toBe(false);
        for (const output of [cli.stdout, mcp.text]) {
          expect(output).toContain(source);
          expect(output).toContain(`# Records`);
          expect(output).toContain(sentence);
          expect(output).not.toMatch(/[\0\uFFFD]/u);
          expect(output).not.toMatch(/MP3|AAC|ffprobe/u);
          if (isHtml) {
            expect(output).toContain('title: Records 陳小明😀');
            expect(output).toContain('|陳小明😀|香港|');
            expect(output).not.toMatch(/<h1>|<p>|<table>|unsafe-marker|javascript:/u);
          } else {
            expect(output).toContain(`# Records\r\n\r\n${sentence}\r\nSecond paragraph.`);
          }
        }
        cliControl ??= withoutSource(cli.stdout);
        mcpControl ??= withoutSource(mcp.text);
        expect(withoutSource(cli.stdout)).toBe(cliControl);
        expect(withoutSource(mcp.text)).toBe(mcpControl);
      }
    });

    test(`${extension}: malformed UTF-16 errors and the same MCP session recovers`, async () => {
      for (const encoding of ['utf16le', 'utf16be'] as const) {
        for (const broken of [
          Buffer.concat([encode('Records\r\n', encoding), Buffer.from([0x41])]),
          encode('Records\r\n\uD800', encoding),
          encode('Records\r\n\uDC00', encoding),
        ]) {
          const source = join(directory, `broken-${id}.${extension}`);
          writeFileSync(source, broken);
          const cli = spawnSync(resolveServerPath(), [source], { encoding: 'utf8' });
          expect(cli.status).not.toBe(0);
          expect(`${cli.stdout}${cli.stderr}`).toMatch(/UTF-16.*re-export/u);
          const mcp = parseToolPayload(await callTool(proc, id++, 'read', { source }));
          expect(mcp.isError).toBe(true);
          expect(mcp.text).toMatch(/UTF-16.*re-export/u);
          const valid = join(directory, `recovered-${id}.${extension}`);
          writeFileSync(valid, encode(isHtml ? html : text, encoding));
          const recovered = parseToolPayload(await callTool(proc, id++, 'read', { source: valid }));
          expect(recovered.isError).toBe(false);
          expect(recovered.text).toContain(sentence);
        }
      }
    });
  }

  test('binary data does not become text because of a BOM or file suffix', async () => {
    for (const bytes of [
      Buffer.from([0x80, 0x81, 0, 1]),
      Buffer.from([0x41, 0xc2]),
      Buffer.from([0xef, 0xbb, 0xbf, 0x80, 0x81, 0, 1]),
      encode('\0\u0001\u0002', 'utf16le'),
    ]) {
      const source = join(directory, `binary-${id}.txt`);
      writeFileSync(source, bytes);
      expect(spawnSync(resolveServerPath(), [source]).status).not.toBe(0);
      expect(parseToolPayload(await callTool(proc, id++, 'read', { source })).isError).toBe(true);
    }
  });
});
