#!/usr/bin/env bun
/** Validate agent-task corpus for ADR-0005 quality parity. */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const manifestPath = join(root, 'docs/specs/agent-task-corpus/manifest.json');
const failures: string[] = [];

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
  schemaVersion: number;
  authority?: string;
  calibration?: { inventedNumericThresholdsForbidden?: boolean };
  fixtures?: Record<string, string>;
  taskFiles?: string[];
  publicTaskFiles?: string[];
  baselineArtifact?: string;
  publicBaselineArtifact?: string;
  publicCorpus?: { manifest?: string };
};

if (manifest.schemaVersion !== 1) failures.push('manifest schemaVersion must be 1');
if (manifest.authority !== 'ADR-0005') failures.push('manifest authority must be ADR-0005');
if (manifest.calibration?.inventedNumericThresholdsForbidden !== true) {
  failures.push('manifest must forbid invented numeric thresholds');
}
for (const [name, fixture] of Object.entries(manifest.fixtures ?? {})) {
  if (!existsSync(join(root, fixture))) failures.push(`fixture missing for ${name}: ${fixture}`);
}

const taskFiles = manifest.taskFiles ?? [];
const publicTaskFiles = manifest.publicTaskFiles ?? [];
if (taskFiles.length < 7) failures.push('manifest must reference at least seven local tasks');
if (publicTaskFiles.length < 3) {
  failures.push('manifest must reference at least three public-url tasks');
}
if (manifest.baselineArtifact) {
  const baselinePath = join(root, 'docs/specs/agent-task-corpus', manifest.baselineArtifact);
  if (!existsSync(baselinePath)) {
    failures.push(`baselineArtifact missing: ${manifest.baselineArtifact}`);
  }
}
if (manifest.publicCorpus?.manifest && !existsSync(join(root, manifest.publicCorpus.manifest))) {
  failures.push(`public corpus manifest missing: ${manifest.publicCorpus.manifest}`);
}

const ids = new Set<string>();
const contractIds = new Set<string>();
const validateTaskFile = (rel: string, requirePublic: boolean) => {
  const path = join(root, 'docs/specs/agent-task-corpus', rel);
  if (!existsSync(path)) {
    failures.push(`task file missing: ${rel}`);
    return;
  }
  const task = JSON.parse(readFileSync(path, 'utf8')) as {
    id?: string;
    scope?: string;
    publicCaseId?: string;
    tool?: string;
    fixture?: string;
    input?: unknown;
    acceptance?: Record<string, unknown>;
    contractIds?: string[];
  };
  if (!task.id) failures.push(`${rel} missing id`);
  if (task.id && ids.has(task.id)) failures.push(`duplicate task id ${task.id}`);
  if (task.id) ids.add(task.id);
  if (!['read_pdf', 'search_pdf', 'pdf_compare', 'pdf_evidence'].includes(String(task.tool))) {
    failures.push(`${rel} has invalid tool`);
  }
  if (!task.input || typeof task.input !== 'object') failures.push(`${rel} input must be object`);
  if (!task.acceptance || typeof task.acceptance !== 'object') {
    failures.push(`${rel} acceptance must be object`);
  }
  if (!Array.isArray(task.contractIds) || task.contractIds.length === 0) {
    failures.push(`${rel} must link contractIds`);
  }
  for (const id of task.contractIds ?? []) contractIds.add(id);
  if (requirePublic) {
    if (task.scope !== 'public-url') failures.push(`${rel} must set scope=public-url`);
    if (!task.publicCaseId) failures.push(`${rel} must set publicCaseId`);
  } else if (task.fixture && !existsSync(join(root, task.fixture))) {
    failures.push(`${rel} fixture missing`);
  }
};

for (const rel of taskFiles) validateTaskFile(rel, false);
for (const rel of publicTaskFiles) validateTaskFile(rel, true);

for (const id of contractIds) {
  const contractPath = join(root, 'docs/specs/semantic-contracts', `${id}.json`);
  if (!existsSync(contractPath)) failures.push(`task corpus links missing contract: ${id}`);
}

const requiredTaskIds = [
  'extract-passage-sample',
  'table-value-selectable',
  'visual-candidates-provider-independent',
  'ocr-text-layer-mock-provider',
  'visual-enrichment-document-map-fusion',
];
for (const requiredId of requiredTaskIds) {
  if (!ids.has(requiredId)) failures.push(`missing required task id: ${requiredId}`);
}
if (![...ids].some((id) => id.startsWith('public-'))) {
  failures.push('missing public-url task ids');
}

if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log(
  `[check-agent-task-corpus] PASS ${taskFiles.length} local + ${publicTaskFiles.length} public-url tasks under ADR-0005`
);
