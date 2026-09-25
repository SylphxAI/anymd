#!/usr/bin/env bun
/**
 * Brand pack plan — brand-sole readiness (no dual-publish as end state).
 */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dir, '..');
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  name?: string;
  version?: string;
  bin?: Record<string, string>;
};
const server = existsSync(join(root, 'server.json'))
  ? (JSON.parse(readFileSync(join(root, 'server.json'), 'utf8')) as { title?: string; name?: string })
  : {};

const plan = {
  repoRoot: root,
  canonicalName: '@sylphx/anymd',
  actualName: pkg.name,
  version: pkg.version,
  brandBin: pkg.bin?.anymd,
  marketplaceTitle: server.title,
  marketplaceName: server.name,
  hasSkill: existsSync(join(root, 'skills/anymd/SKILL.md')),
  brandPublishDoc: existsSync(join(root, 'docs/BRAND_PUBLISH.md')),
  brandSole: pkg.name === '@sylphx/anymd' && pkg.bin?.anymd === './dist/runtime-entry.js',
  transitionalDeprecated: true,
  npmAuthRequiredForLivePublish: true,
  ok: false as boolean,
};
plan.ok = Boolean(
  plan.brandSole &&
    server.title === 'anymd' &&
    plan.hasSkill &&
    plan.brandPublishDoc &&
    plan.version
);
console.log(JSON.stringify(plan, null, 2));
process.exit(plan.ok ? 0 : 1);
