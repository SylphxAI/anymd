import { beforeAll, describe, expect, it } from 'bun:test';
import { execSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { resolveCliPath, resolveServerPath } from './utils/cargoBinaries.js';

const repoRoot = path.resolve(import.meta.dirname, '..');
const fixturesRoot = path.join(repoRoot, 'test/fixtures');
const goldenPath = path.join(fixturesRoot, 'read-pdf-golden.json');
const rustCliBin = resolveCliPath();
const samplePdf = path.join(fixturesRoot, 'sample.pdf');

type GoldenCase = {
  id: string;
  fixture: string;
  input: Record<string, unknown>;
  expects: {
    error?: boolean;
    code?: string;
    message_contains?: string;
    route?: string;
    payload?: Record<string, unknown>;
    evidence?: Record<string, unknown>;
  };
};

type GoldenManifest = {
  profile: string;
  cases: GoldenCase[];
};

const normalizePayload = (payload: Record<string, unknown>) => {
  const normalized = structuredClone(payload);
  if (Array.isArray(normalized.results)) {
    normalized.results = normalized.results.map((result) => {
      if (!result || typeof result !== 'object') {
        return result;
      }
      const entry = { ...(result as Record<string, unknown>) };
      if (typeof entry.source === 'string') {
        entry.source = path.relative(fixturesRoot, entry.source).split(path.sep).join('/');
      }
      if (entry.data && typeof entry.data === 'object') {
        const data = { ...(entry.data as Record<string, unknown>) };
        data.fullText = undefined;
        data.full_text = undefined;
        if (data.info && typeof data.info === 'object') {
          const info = { ...(data.info as Record<string, unknown>) };
          info.text_chars = undefined;
          info.textChars = undefined;
          data.info = info;
        }
        entry.data = data;
      }
      return entry;
    });
  }
  return normalized;
};

const buildRequestInput = (fixture: string, input: Record<string, unknown>) => {
  const request = structuredClone(input);
  if (!Array.isArray(request.sources)) {
    request.sources = [{ path: path.join(fixturesRoot, fixture) }];
  } else {
    request.sources = request.sources.map((source) => {
      if (!source || typeof source !== 'object') {
        return source;
      }
      const entry = { ...(source as Record<string, unknown>) };
      if (typeof entry.path === 'string' && !path.isAbsolute(entry.path)) {
        entry.path = path.join(fixturesRoot, entry.path);
      }
      return entry;
    });
  }
  return request;
};

const invokeCli = (fixture: string, input: Record<string, unknown>) => {
  const probe = spawnSync(rustCliBin, [], {
    cwd: repoRoot,
    encoding: 'utf8',
    input: JSON.stringify({
      tool: 'read_pdf',
      input: buildRequestInput(fixture, input),
    }),
    timeout: 30_000,
  });

  return {
    status: probe.status,
    stdout: probe.stdout,
    stderr: probe.stderr,
    envelope: probe.status === 0 ? (JSON.parse(probe.stdout) as Record<string, unknown>) : null,
  };
};

const parseCliPayload = (envelope: Record<string, unknown>) => {
  const text = (envelope.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]
    ?.text;
  if (!text) {
    throw new Error('CLI envelope missing read_pdf payload text');
  }
  return JSON.parse(text) as Record<string, unknown>;
};

describe('read_pdf golden parity', () => {
  let golden: GoldenManifest;

  beforeAll(() => {
    golden = JSON.parse(readFileSync(goldenPath, 'utf8')) as GoldenManifest;
    expect(golden.profile).toBe('pdf_reader_read_pdf_golden');

    execSync(
      'cargo build --release -p pdf-reader-core -p pdf-reader-cli -p pdf-reader-mcp-server',
      {
        cwd: repoRoot,
        stdio: 'pipe',
        timeout: 300_000,
      }
    );
  }, 300_000);

  for (const caseId of [
    'sample-metadata-on',
    'sample-minimal-route',
    'sample-full-text',
    'missing-file',
    'empty-sources',
    'url-source',
  ] as const) {
    it(`Rust CLI matches golden contract for ${caseId}`, () => {
      const caseEntry = golden.cases.find((entry) => entry.id === caseId);
      expect(caseEntry).toBeDefined();

      const probe = invokeCli(caseEntry?.fixture, caseEntry?.input);

      if (caseEntry?.expects.error) {
        expect(probe.status).toBe(0);
        const envelope = probe.envelope as { status?: string; code?: string; message?: string };
        expect(envelope.status).toBe('error');
        expect(envelope.code).toBe(caseEntry?.expects.code);
        expect(envelope.message?.toLowerCase()).toContain(
          caseEntry?.expects.message_contains?.toLowerCase()
        );
        return;
      }

      if (!existsSync(samplePdf)) {
        return;
      }

      expect(probe.status).toBe(0);
      const envelope = probe.envelope as { status?: string; tool?: string };
      expect(envelope.status).toBe('ok');
      expect(envelope.tool).toBe('read_pdf');

      const actual = normalizePayload(parseCliPayload(envelope));
      const expected = normalizePayload(caseEntry?.expects.payload as Record<string, unknown>);

      expect(actual.profile).toBe(expected.profile);
      const actualResults = actual.results as Array<Record<string, unknown>>;
      const expectedResults = expected.results as Array<Record<string, unknown>>;
      expect(actualResults[0]?.success).toBe(expectedResults[0]?.success);
      expect((actualResults[0]?.data as Record<string, unknown> | undefined)?.route).toBe(
        caseEntry?.expects.route
      );
      const expectedEngine = (
        expectedResults[0]?.data as { engine?: { name?: string; version?: string } } | undefined
      )?.engine;
      if (expectedEngine !== undefined) {
        expect(
          (actualResults[0]?.data as { engine?: { name?: string; version?: string } } | undefined)
            ?.engine
        ).toEqual(expectedEngine);
      }

      const actualInfo = (actualResults[0]?.data as { info?: Record<string, unknown> } | undefined)
        ?.info;
      const expectedInfo = (
        expectedResults[0]?.data as { info?: Record<string, unknown> } | undefined
      )?.info;
      if (expectedInfo) {
        for (const [key, value] of Object.entries(expectedInfo)) {
          expect(actualInfo?.[key]).toEqual(value);
        }
      }

      const fullTextNeedle = (
        expectedResults[0]?.data as { full_text_contains?: string } | undefined
      )?.full_text_contains;
      if (fullTextNeedle) {
        const rawPayload = parseCliPayload(envelope);
        const rawResults = rawPayload.results as Array<{
          data?: { fullText?: string; full_text?: string };
        }>;
        const fullText = rawResults[0]?.data?.fullText ?? rawResults[0]?.data?.full_text ?? '';
        expect(fullText).toContain(fullTextNeedle);
      }
    });
  }

  it('Rust CLI sample-metadata-on payload includes positive page count', () => {
    if (!existsSync(samplePdf)) {
      return;
    }

    const caseEntry = golden.cases.find((entry) => entry.id === 'sample-metadata-on');
    expect(caseEntry).toBeDefined();

    const probe = invokeCli(caseEntry?.fixture, caseEntry?.input);
    expect(probe.status).toBe(0);

    const payload = parseCliPayload(probe.envelope as Record<string, unknown>);
    const firstResult = (
      payload.results as Array<{ data?: { numPages?: number; num_pages?: number } }>
    )[0];
    const numPages = firstResult?.data?.numPages ?? firstResult?.data?.num_pages ?? 0;
    expect(numPages).toBeGreaterThan(0);
  });
});
