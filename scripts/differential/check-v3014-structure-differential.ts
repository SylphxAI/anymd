#!/usr/bin/env bun
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  callTool,
  ensureProductionArtifacts,
  initializeSession,
  parseToolPayload,
  spawnProductionMcp,
} from "../../test/production/mcpContract.helpers.ts";
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  assertStructureMutationSensitivity,
  canonicalEqual,
  canonicalInspect,
  canonicalReadStructure,
} from "./v3014-structure-projection.ts";
const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "../..");
const fixtureDir = join(repoRoot, "test/fixtures/differential");
const corpusPath = join(scriptDir, "fixtures/v3014-structure-corpus.json");
const oraclePath = join(scriptDir, "fixtures/v3014-structure-oracle.json");
const manifestPath = join(scriptDir, "fixtures/v3014-structure-fixtures.json");
const cli = join(resolveCargoReleaseDir(repoRoot), "pdf-reader-cli");
const corpus = JSON.parse(readFileSync(corpusPath, "utf8"));
const oracle = JSON.parse(readFileSync(oraclePath, "utf8"));
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
const sha = (v: Uint8Array | string) =>
  createHash("sha256").update(v).digest("hex");
const git = (...args: string[]) => {
  const r = spawnSync("git", args, { cwd: repoRoot });
  if (r.status !== 0) throw new Error(String(r.stderr));
  return r.stdout;
};
if (
  git("rev-list", "-n", "1", oracle.baseline.tag).toString().trim() !==
  oracle.baseline.commit
)
  throw new Error("baseline moved");
if (
  git("rev-parse", `${oracle.baseline.commit}^{tree}`).toString().trim() !==
  oracle.baseline.tree
)
  throw new Error("baseline tree mismatch");
if (
  sha(git("show", `${oracle.baseline.commit}:bun.lock`)) !==
  oracle.baseline.bunLockSha256
)
  throw new Error("lock mismatch");
for (const [path, digest] of Object.entries(oracle.baseline.entrypointSha256)) {
  if (sha(git("show", `${oracle.baseline.commit}:${path}`)) !== digest)
    throw new Error(`entrypoint mismatch ${path}`);
}
for (const fixture of manifest.fixtures) {
  const bytes = readFileSync(join(repoRoot, fixture.path));
  if (bytes.length !== fixture.bytes || sha(bytes) !== fixture.sha256)
    throw new Error(`fixture mismatch ${fixture.path}`);
}
if (
  sha(readFileSync(join(repoRoot, manifest.generator))) !==
  manifest.generatorSha256
)
  throw new Error("generator mismatch");
if (corpus.cases.length !== 10)
  throw new Error("structure corpus must contain 10 cases");
const packageJson = JSON.parse(
  readFileSync(join(repoRoot, "package.json"), "utf8")
);
// Oracle/test dependency only under sole-Rust production (ADR-0006 / 4.0.1).
const declaredPdfjs =
  packageJson.devDependencies?.["pdfjs-dist"] ??
  packageJson.dependencies?.["pdfjs-dist"];
if (declaredPdfjs !== "^6.0.227") {
  throw new Error(
    "candidate pdfjs-dist oracle dependency is not 6.0.227 (expected in devDependencies)"
  );
}
if (packageJson.dependencies?.["pdfjs-dist"]) {
  throw new Error(
    "pdfjs-dist must not remain a production dependency under sole-Rust production packaging"
  );
}
const installedPdfjs = JSON.parse(
  readFileSync(join(repoRoot, "node_modules/pdfjs-dist/package.json"), "utf8")
);
if (
  oracle.baseline.pdfjs !== "6.0.227" ||
  installedPdfjs.version !== "6.0.227"
) {
  throw new Error("frozen pdfjs-dist runtime is not exactly 6.0.227");
}
const harness = {
  baselineRunnerSha256: sha(
    readFileSync(join(scriptDir, "v3014-structure-baseline-runner.ts"))
  ),
  normalizerRunnerSha256: sha(
    readFileSync(
      join(scriptDir, "v3014-structure-normalizer-baseline-runner.ts")
    )
  ),
  projectionSha256: sha(
    readFileSync(join(scriptDir, "v3014-structure-projection.ts"))
  ),
  normalizerOracleSha256: sha(
    readFileSync(
      join(scriptDir, "fixtures/v3014-structure-normalizer-oracle.json")
    )
  ),
};
if (!canonicalEqual(harness, oracle.harness))
  throw new Error("harness digest mismatch");
const normalizerOracleBytes = readFileSync(
  join(scriptDir, "fixtures/v3014-structure-normalizer-oracle.json")
);
if (
  sha(Buffer.concat([normalizerOracleBytes, Buffer.from("\n")])) ===
  oracle.harness.normalizerOracleSha256
)
  throw new Error("normalizer oracle digest is not mutation-sensitive");
const materialize = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
        key === "fixture" ? "path" : key,
        key === "fixture"
          ? join(fixtureDir, String(entry))
          : materialize(entry),
      ])
    );
  return value;
};
const actual: Record<string, unknown> = {};
for (const entry of corpus.cases) {
  const r = spawnSync(cli, [], {
    cwd: repoRoot,
    encoding: "utf8",
    input: JSON.stringify({
      tool: "read_pdf",
      input: materialize(entry.input),
    }),
    maxBuffer: 20 * 1024 * 1024,
  });
  if (r.status !== 0 || !r.stdout)
    throw new Error(`Rust CLI failed ${entry.id}: ${r.stderr}`);
  const envelope = JSON.parse(r.stdout);
  const payload = JSON.parse(envelope.result.content[0].text);
  actual[entry.id] = canonicalReadStructure(payload.results[0].data);
}
ensureProductionArtifacts("pure-rust");
const proc = spawnProductionMcp({
  PDF_READER_ENGINE_MODE: "pure-rust",
  MCP_PDF_OCR_COMMAND: "",
  MCP_PDF_OCR_PRESET: "",
});
try {
  await initializeSession(proc, "v3014-structure-inspect-differential");
  const response = await callTool(
    proc,
    2,
    "pdf_evidence",
    {
      operation: "inspect",
      sources: [{ path: join(fixtureDir, "v3014-structure-v1.pdf") }],
      sample_pages: 2,
    },
    90_000
  );
  const payload = parseToolPayload(response);
  if (payload.isError) throw new Error(`Rust inspect failed: ${payload.text}`);
  const inspected = JSON.parse(payload.text).results?.[0];
  if (!inspected?.success)
    throw new Error(`Rust inspect source failed: ${inspected?.error}`);
  actual["inspect-tagged"] = canonicalInspect(inspected.data);
} finally {
  proc.kill("SIGTERM");
}
assertStructureMutationSensitivity(oracle.expectations, actual);
const pass = canonicalEqual(oracle.expectations, actual);
const result = {
  schemaVersion: 1,
  profile: "pdf_reader_v3014_structure_result",
  candidateSha: git("rev-parse", "HEAD").toString().trim(),
  caseCount: corpus.cases.length + 1,
  passed: pass ? corpus.cases.length + 1 : 0,
  skipped: 0,
  pass,
  failures: pass ? [] : [{ expected: oracle.expectations, actual }],
};
console.log(JSON.stringify(result, null, 2));
if (!pass) process.exit(1);
