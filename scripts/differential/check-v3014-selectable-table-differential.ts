#!/usr/bin/env bun

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalSelectableTableResult,
  SELECTABLE_TABLE_MUTATION_MANIFEST,
  type Json,
} from "./v3014-selectable-table-projection.ts";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "../..");
const fixtureDir = join(repoRoot, "test/fixtures/differential");
const corpusPath = join(
  scriptDir,
  "fixtures/v3014-selectable-table-corpus.json"
);
const oraclePath = join(
  scriptDir,
  "fixtures/v3014-selectable-table-oracle.json"
);
const manifestPath = join(
  scriptDir,
  "fixtures/v3014-selectable-table-fixture.json"
);
const runnerPath = join(scriptDir, "v3014-selectable-table-baseline-runner.ts");
const projectionPath = join(scriptDir, "v3014-selectable-table-projection.ts");
const generatorPath = join(
  scriptDir,
  "generate-v3014-selectable-table-fixture.ts"
);
const cli = join(resolveCargoReleaseDir(repoRoot), "pdf-reader-cli");
const outputFlag = process.argv.indexOf("--output");
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;
type Case = { id: string; input: Record<string, unknown> };
type Oracle = {
  baseline: Record<string, unknown>;
  expectations: Record<string, Json>;
};
const corpus = JSON.parse(readFileSync(corpusPath, "utf8")) as {
  cases: Case[];
};
const oracle = JSON.parse(readFileSync(oraclePath, "utf8")) as Oracle;
const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
  fixture: { path: string; bytes: number; sha256: string };
  admittedFixture: {
    path: string;
    bytes: number;
    sha256: string;
    itemCount: number;
  };
  hostileFixture: {
    path: string;
    bytes: number;
    sha256: string;
    itemCount: number;
  };
};
const sha = (x: Uint8Array | string): string =>
  createHash("sha256").update(x).digest("hex");
const git = (...args: string[]): Buffer => {
  const r = spawnSync("git", args, { cwd: repoRoot });
  if (r.status !== 0) throw new Error(r.stderr.toString());
  return r.stdout;
};
const canon = (v: Json): Json =>
  Array.isArray(v)
    ? v.map(canon)
    : v && typeof v === "object"
    ? Object.fromEntries(
        Object.entries(v)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([k, x]) => [k, canon(x)])
      )
    : v;
const same = (a: unknown, b: unknown): boolean =>
  JSON.stringify(canon(a as Json)) === JSON.stringify(canon(b as Json));
const expectedIds = [
  "tables-only-public-surface",
  "tables-hidden-downstream-dependencies",
  "tables-exposed-downstream-linkage",
  "tables-cross-page-continuation",
  "tables-selected-pages-dedupe-sort",
  "tables-no-grid-omission",
];
function authority(): Record<string, string> {
  const b = oracle.baseline;
  const commit = git("rev-list", "-n", "1", String(b.tag)).toString().trim();
  if (commit !== b.commit) throw new Error("baseline tag moved");
  const tree = git("rev-parse", `${commit}^{tree}`).toString().trim();
  if (tree !== b.tree) throw new Error("baseline tree mismatch");
  if (sha(git("show", `${commit}:bun.lock`)) !== b.bunLockSha256)
    throw new Error("baseline lock mismatch");
  for (const [p, e, n] of [
    [runnerPath, b.runnerSha256, "runner"],
    [projectionPath, b.projectionSha256, "projection"],
    [generatorPath, b.generatorSha256, "generator"],
    [corpusPath, b.corpusSha256, "corpus"],
    [manifestPath, b.fixtureManifestSha256, "manifest"],
  ] as const)
    if (sha(readFileSync(p)) !== e)
      throw new Error(`selectable-table ${n} mismatch`);
  for (const [p, e] of Object.entries(
    b.entrypointSha256 as Record<string, string>
  ))
    if (sha(git("show", `${commit}:${p}`)) !== e)
      throw new Error(`baseline source mismatch: ${p}`);
  for (const [f, e] of [
    [manifest.fixture, b.fixtureSha256],
    [manifest.admittedFixture, b.admittedFixtureSha256],
    [manifest.hostileFixture, b.hostileFixtureSha256],
  ] as const) {
    const bytes = readFileSync(join(repoRoot, f.path));
    if (bytes.length !== f.bytes || sha(bytes) !== f.sha256 || f.sha256 !== e)
      throw new Error(`fixture mismatch: ${f.path}`);
  }
  if (manifest.admittedFixture.itemCount !== 4096)
    throw new Error("admitted fixture must be exact cap");
  if (manifest.hostileFixture.itemCount !== 4097)
    throw new Error("hostile fixture must be exact cap+1");
  if (
    !same(
      corpus.cases.map((x) => x.id),
      expectedIds
    ) ||
    !same(Object.keys(oracle.expectations).sort(), [...expectedIds].sort())
  )
    throw new Error("exact six-case corpus required");
  return {
    baselineCommit: commit,
    baselineTree: tree,
    oracleSha256: sha(readFileSync(oraclePath)),
    corpusSha256: sha(readFileSync(corpusPath)),
    runnerSha256: sha(readFileSync(runnerPath)),
    projectionSha256: sha(readFileSync(projectionPath)),
    generatorSha256: sha(readFileSync(generatorPath)),
    fixtureManifestSha256: sha(readFileSync(manifestPath)),
    fixtureSha256: manifest.fixture.sha256,
    hostileFixtureSha256: manifest.hostileFixture.sha256,
  };
}
const materialize = (value: unknown): unknown =>
  Array.isArray(value)
    ? value.map(materialize)
    : value && typeof value === "object"
    ? Object.fromEntries(
        Object.entries(value as Record<string, unknown>).map(([k, v]) => [
          k === "fixture" ? "path" : k,
          k === "fixture" && typeof v === "string"
            ? join(fixtureDir, v)
            : materialize(v),
        ])
      )
    : value;
const runCase = (
  input: Record<string, unknown>,
  label: string
): Record<string, unknown> => {
  const r = spawnSync(cli, [], {
    cwd: repoRoot,
    encoding: "utf8",
    input: JSON.stringify({ tool: "read_pdf", input: materialize(input) }),
    maxBuffer: 160 * 1024 * 1024,
    timeout: 120000,
  });
  if (r.status !== 0) throw new Error(r.stderr || `${label} exited nonzero`);
  const env = JSON.parse(r.stdout) as {
    result?: { content?: Array<{ text?: string }> };
  };
  const text = env.result?.content?.[0]?.text;
  if (!text) throw new Error(`${label} lacks structured text`);
  const payload = JSON.parse(text) as {
    results: Array<{ success: boolean; data: Record<string, unknown> }>;
  };
  if (!payload.results[0]?.success) throw new Error(`${label} failed`);
  return payload.results[0].data;
};
const auth = authority();
if (!existsSync(cli)) throw new Error("missing release Rust CLI");
let subprocessNonzeroRejected = false;
try {
  const p = spawnSync(process.execPath, ["-e", "process.exit(19)"]);
  if (p.status !== 0) throw new Error("rejected");
} catch {
  subprocessNonzeroRejected = true;
}
if (!subprocessNonzeroRejected)
  throw new Error("nonzero subprocess not rejected");
const raw: Record<string, Record<string, unknown>> = {};
const observations: Record<string, Json> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const data = runCase(entry.input, entry.id);
  raw[entry.id] = data;
  const actual = canonicalSelectableTableResult(data);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected))
    failures.push({ id: entry.id, expected, actual });
}
const leafPaths = (
  value: Json,
  prefix: Array<string | number> = []
): Array<Array<string | number>> =>
  Array.isArray(value)
    ? value.flatMap((entry, index) => leafPaths(entry, [...prefix, index]))
    : value && typeof value === "object"
    ? Object.entries(value).flatMap(([key, entry]) =>
        leafPaths(entry, [...prefix, key])
      )
    : [prefix];
const mutateAt = (value: Json, path: Array<string | number>): Json => {
  const mutated = structuredClone(value);
  let cursor = mutated as Json;
  for (const segment of path.slice(0, -1))
    cursor = (cursor as never)[segment as never];
  const key = path.at(-1)!;
  const original = (cursor as never)[key as never] as Json;
  const replacement: Json =
    typeof original === "string"
      ? `${original}-mutated`
      : typeof original === "number"
      ? original + 1
      : typeof original === "boolean"
      ? !original
      : "mutated";
  (cursor as never)[key as never] = replacement as never;
  return mutated;
};
let leafMutationCount = 0;
for (const [caseId, expectation] of Object.entries(oracle.expectations)) {
  const paths = leafPaths(expectation);
  const missed = paths.filter((path) =>
    same(mutateAt(expectation, path), expectation)
  );
  if (missed.length > 0)
    throw new Error(
      `canonical comparison missed ${missed.length} leaf mutations in ${caseId}`
    );
  leafMutationCount += paths.length;
}
const only = raw["tables-only-public-surface"]!;
const hidden = raw["tables-hidden-downstream-dependencies"]!;
const exposed = raw["tables-exposed-downstream-linkage"]!;
const cross = raw["tables-cross-page-continuation"]!;
const noGrid = raw["tables-no-grid-omission"]!;
const getTables = (v: Record<string, unknown>) =>
  (v.tables ?? []) as Array<Record<string, unknown>>;
const getTableEls = (v: Record<string, unknown>) =>
  ((v.elements ?? []) as Array<Record<string, unknown>>).filter(
    (x) => x.type === "table"
  );
const getTableChunks = (v: Record<string, unknown>) =>
  ((v.chunks ?? []) as Array<Record<string, unknown>>).filter(
    (x) => x.strategy === "table"
  );
const table = getTables(only)[0]!;
const quality = table.quality as Record<string, unknown>;
const exposedTable = getTables(exposed)[0]!;
const exposedQuality = exposedTable.quality as Record<string, unknown>;
const exposedTrustMessages = (
  ((exposed.trust_report as Record<string, unknown>)?.signals ?? []) as Array<
    Record<string, unknown>
  >
)
  .filter((signal) => signal.type === "table_quality")
  .map((signal) => signal.message);
const semanticProof = {
  exactSparseGrid:
    getTables(only).length === 1 &&
    table.rowCount === 3 &&
    table.colCount === 3 &&
    table.cellCount === undefined &&
    (table.cells as unknown[]).length === 9,
  qualityWarnings:
    Array.isArray(quality.warnings) &&
    (quality.warnings as unknown[]).length === 2,
  exactPageContentMergedCells:
    exposedQuality.mergedCellCandidateCount === 5 &&
    ((exposedTable.cells as Array<Record<string, unknown>>) ?? []).filter(
      (cell) => cell.colSpan === 2
    ).length === 5 &&
    ((exposedQuality.signals as unknown[]) ?? []).includes(
      "merged_cell_candidates"
    ) &&
    ((exposedQuality.warnings as unknown[]) ?? []).length === 3,
  exactOrderedTrustSignals:
    same(exposedTrustMessages, [
      "Detected empty inferred cells; table may contain sparse or merged structure.",
      "Detected cells whose text boxes cross column boundaries; spans are inferred.",
      "Some table cells lack bounding boxes; verify the table with region crops when cell-level evidence matters.",
    ]),
  hiddenTopLevelDependencies:
    !Object.hasOwn(only, "elements") &&
    !Object.hasOwn(only, "chunks") &&
    !Object.hasOwn(only, "document_ast") &&
    !Object.hasOwn(only, "document_map") &&
    !Object.hasOwn(only, "trust_report"),
  downstreamTablesHidden:
    !Object.hasOwn(hidden, "tables") &&
    getTableEls(hidden).length === 0 &&
    getTableChunks(hidden).length === 0,
  exposedLinkage:
    getTables(exposed).length === 1 &&
    getTableEls(exposed).length === 1 &&
    getTableChunks(exposed).length === 1 &&
    JSON.stringify(exposed.document_ast).includes('"type":"table"') &&
    JSON.stringify(exposed.document_map).includes('"table_structure"') &&
    JSON.stringify(exposed.trust_report).includes('"table_quality"'),
  markdownHtml:
    typeof exposed.markdown === "string" &&
    exposed.markdown.includes("| Metric | Value | Region |") &&
    typeof exposed.html === "string" &&
    exposed.html.includes("<table"),
  selectedPages:
    getTables(raw["tables-selected-pages-dedupe-sort"]!)
      .map((x) => x.page)
      .join(",") === "1,2",
  noGridOmission:
    getTables(noGrid).length === 0 &&
    getTableEls(noGrid).length === 0 &&
    getTableChunks(noGrid).length === 0,
  htmlEscaping:
    typeof cross.html === "string" &&
    cross.html.includes("Percent &amp; &quot;rate&#39;s&quot;"),
};
const crossTables = getTables(cross);
const continuationProof = {
  twoTables: crossTables.length === 2,
  groupLinked:
    crossTables[0]?.continuation !== undefined &&
    crossTables[1]?.continuation !== undefined &&
    (crossTables[0]!.continuation as Record<string, unknown>).nextTableId ===
      "p2-table-1" &&
    (crossTables[1]!.continuation as Record<string, unknown>)
      .previousTableId === "p1-table-1",
  sameGroup:
    (crossTables[0]?.continuation as Record<string, unknown>)?.groupId ===
    (crossTables[1]?.continuation as Record<string, unknown>)?.groupId,
  downstreamIds:
    JSON.stringify(cross.document_ast).includes("p1-table-1") &&
    JSON.stringify(cross.document_ast).includes("p2-table-1") &&
    JSON.stringify(cross.document_map).includes("p2-table-1"),
};
const semanticProofPass = Object.values(semanticProof).every(Boolean);
const continuationProofPass = Object.values(continuationProof).every(Boolean);
const admitted = runCase(
  {
    sources: [
      { path: join(repoRoot, manifest.admittedFixture.path), pages: [1] },
    ],
    auto: false,
    include_tables: true,
  },
  "admitted exact cap"
);
const hostile = runCase(
  {
    sources: [
      { path: join(repoRoot, manifest.hostileFixture.path), pages: [1] },
    ],
    auto: false,
    include_tables: true,
  },
  "hostile cap+1"
);
const hostileWarnings = (hostile.warnings ?? []) as string[];
const admittedWarnings = (admitted.warnings ?? []) as string[];
const resourceBoundProof = {
  exactCapItemCount: manifest.admittedFixture.itemCount,
  itemCount: manifest.hostileFixture.itemCount,
  cap: 4096,
  exactCapAccepted:
    getTables(admitted).length > 0 &&
    !admittedWarnings.some((warning) => warning.includes("admission limit")),
  zeroTables: getTables(hostile).length === 0,
  exactWarning: hostileWarnings.includes(
    "Selectable table extraction skipped page 1: spatial grid exceeds the Rust admission limit."
  ),
};
if (Object.values(resourceBoundProof).some((x) => x === false))
  throw new Error(
    `hostile bound proof failed: ${JSON.stringify(resourceBoundProof)}`
  );
const reject = (
  mutate: (v: Record<string, unknown>) => void,
  label: string,
  source = exposed
): void => {
  const v = structuredClone(source);
  mutate(v);
  try {
    canonicalSelectableTableResult(v);
  } catch {
    return;
  }
  throw new Error(`strict projection accepted ${label}`);
};
const pathTokens = (path: string): Array<string | number> =>
  [...path.matchAll(/([^.\[\]]+)|\[(\d+)\]/gu)].map((match) =>
    match[2] === undefined ? match[1]! : Number(match[2])
  );
const rawTokens = (
  source: Record<string, unknown>,
  path: string
): Array<string | number> => {
  const tokens = pathTokens(path);
  if (tokens[0] === "trust_table_signals" && tokens[1] === 0) {
    const signals = (source.trust_report as Record<string, unknown>)
      .signals as Array<Record<string, unknown>>;
    tokens.splice(
      0,
      2,
      "trust_report",
      "signals",
      signals.findIndex((entry) => entry.type === "table_quality")
    );
  }
  if (tokens[0] === "ast_tables" && tokens[1] === 0) {
    const findTablePath = (
      value: unknown,
      prefix: Array<string | number> = []
    ): Array<string | number> | undefined => {
      if (Array.isArray(value)) {
        for (let index = 0; index < value.length; index += 1) {
          const found = findTablePath(value[index], [...prefix, index]);
          if (found) return found;
        }
        return undefined;
      }
      if (!value || typeof value !== "object") return undefined;
      const record = value as Record<string, unknown>;
      if (record.type === "table") return prefix;
      for (const [key, entry] of Object.entries(record)) {
        const found = findTablePath(entry, [...prefix, key]);
        if (found) return found;
      }
      return undefined;
    };
    const found = findTablePath(source.document_ast);
    if (!found) throw new Error(`mutation target missing: ${path}`);
    tokens.splice(0, 2, "document_ast", ...found);
  }
  if (tokens[0] === "map_table_linkage") tokens[0] = "document_map";
  if (tokens[0] === "elements" && tokens[1] === 0) {
    const elements = source.elements as Array<Record<string, unknown>>;
    tokens[1] = elements.findIndex((entry) => entry.type === "table");
  }
  if (tokens[0] === "chunks" && tokens[1] === 0) {
    const chunks = source.chunks as Array<Record<string, unknown>>;
    tokens[1] = chunks.findIndex((entry) => entry.strategy === "table");
  }
  if (tokens.includes(-1)) throw new Error(`mutation target missing: ${path}`);
  return tokens;
};
const targetAt = (
  source: Record<string, unknown>,
  path: string
): { parent: Record<string | number, unknown>; key: string | number } => {
  const tokens = rawTokens(source, path);
  const key = tokens.pop();
  if (key === undefined) throw new Error(`empty mutation path: ${path}`);
  let cursor: unknown = source;
  for (const token of tokens) {
    if (!cursor || typeof cursor !== "object")
      throw new Error(`mutation target is not traversable: ${path}`);
    cursor = (cursor as Record<string | number, unknown>)[token];
  }
  if (!cursor || typeof cursor !== "object")
    throw new Error(`mutation parent missing: ${path}`);
  return { parent: cursor as Record<string | number, unknown>, key };
};
let wrongPrimitiveTypeProbeCount = 0;
for (const path of SELECTABLE_TABLE_MUTATION_MANIFEST.wrongPrimitiveTypes) {
  reject((value) => {
    const { parent, key } = targetAt(value, path);
    const original = parent[key];
    parent[key] = Array.isArray(original)
      ? "not-an-array"
      : typeof original === "number"
      ? "not-a-number"
      : typeof original === "boolean"
      ? "not-a-boolean"
      : 7;
  }, `wrong primitive type at ${path}`);
  wrongPrimitiveTypeProbeCount += 1;
}
let unexpectedFieldProbeCount = 0;
for (const path of SELECTABLE_TABLE_MUTATION_MANIFEST.unexpectedFields) {
  const source = path.includes("continuation") ? cross : exposed;
  reject((value) => {
    const { parent, key } = targetAt(value, path);
    const target = parent[key];
    if (!target || typeof target !== "object" || Array.isArray(target))
      throw new Error(`unexpected-field target is not an object: ${path}`);
    (target as Record<string, unknown>).__unexpected = true;
  }, `unexpected field at ${path}`, source);
  unexpectedFieldProbeCount += 1;
}
let requiredOmissionProbeCount = 0;
for (const path of SELECTABLE_TABLE_MUTATION_MANIFEST.requiredOmissions) {
  reject((value) => {
    const { parent, key } = targetAt(value, path);
    delete parent[key];
  }, `required omission at ${path}`);
  requiredOmissionProbeCount += 1;
}
let privateLeakProbeCount = 0;
for (const key of SELECTABLE_TABLE_MUTATION_MANIFEST.privateLeakage) {
  const value = structuredClone(exposed);
  const baseline = canonicalSelectableTableResult(value);
  value[key] = { leaked: true };
  if (same(canonicalSelectableTableResult(value), baseline))
    throw new Error(`private leakage probe was invisible: ${key}`);
  privateLeakProbeCount += 1;
}
let dependencyPresenceProbeCount = 0;
for (const key of SELECTABLE_TABLE_MUTATION_MANIFEST.dependencyPresence) {
  const value = structuredClone(exposed);
  const baseline = canonicalSelectableTableResult(value);
  if (Object.hasOwn(value, key)) delete value[key];
  else value[key] = { probe: true };
  if (same(canonicalSelectableTableResult(value), baseline))
    throw new Error(`dependency presence probe was invisible: ${key}`);
  dependencyPresenceProbeCount += 1;
}
const mutationSensitive = {
  allClaimedFields:
    leafMutationCount > 0 &&
    wrongPrimitiveTypeProbeCount ===
      SELECTABLE_TABLE_MUTATION_MANIFEST.wrongPrimitiveTypes.length &&
    unexpectedFieldProbeCount ===
      SELECTABLE_TABLE_MUTATION_MANIFEST.unexpectedFields.length &&
    requiredOmissionProbeCount ===
      SELECTABLE_TABLE_MUTATION_MANIFEST.requiredOmissions.length &&
    privateLeakProbeCount ===
      SELECTABLE_TABLE_MUTATION_MANIFEST.privateLeakage.length &&
    dependencyPresenceProbeCount ===
      SELECTABLE_TABLE_MUTATION_MANIFEST.dependencyPresence.length,
  manifestVersion: SELECTABLE_TABLE_MUTATION_MANIFEST.version,
  mutationManifestSha256: sha(
    JSON.stringify(SELECTABLE_TABLE_MUTATION_MANIFEST)
  ),
  leafMutationCount,
  wrongPrimitiveTypeProbeCount,
  unexpectedFieldProbeCount,
  requiredOmissionProbeCount,
  privateLeakProbeCount,
  dependencyPresenceProbeCount,
};
const candidateSha = git("rev-parse", "HEAD").toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha)
  throw new Error(
    `candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`
  );
const report = {
  schemaVersion: 1,
  profile: "pdf_reader_v3014_selectable_table_result",
  candidateSha,
  ...auth,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  subprocessNonzeroRejected,
  mutationSensitive,
  semanticProof,
  continuationProof,
  resourceBoundProof,
  nonClaims: [
    "exact provenance engine label values: Rust truthfully reports pdf-reader-core while TS reports pdfjs",
    "OCR-derived, visual/provider, ML, and general-table detection parity",
    "selectable-table behavior outside the exact six-case corpus",
    "exact-cap/cap+1 admission is a Rust safety proof, not TS semantic equality",
    "full Document AST, Document Map, Trust Report, markdown, HTML, element, and chunk parity outside exact table linkage",
  ],
  pass: failures.length === 0 && semanticProofPass && continuationProofPass,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (!report.pass) process.exit(1);
