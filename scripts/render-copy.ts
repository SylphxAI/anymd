#!/usr/bin/env bun
/**
 * Render public copy from one source: product.json (names, descriptions, keywords,
 * sibling projects). Benchmark numbers are generated separately by bench/leaderboard.py.
 *
 * Generated regions are fenced in Markdown as
 *   <!-- generated:NAME --> … <!-- /generated:NAME -->
 * and in YAML front matter as
 *   # generated:NAME … # /generated:NAME
 * Edit product.json, never the fenced text.
 *
 * Usage:
 *   bun scripts/render-copy.ts                    # rewrite every target
 *   bun scripts/render-copy.ts --check            # fail if any target drifted
 *   bun scripts/render-copy.ts --github           # print the GitHub description/topics commands
 *   bun scripts/render-copy.ts --apply-github     # apply them with gh (REST)
 */
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const REPO = 'SylphxAI/anymd';
const REGISTRY_DESCRIPTION_LIMIT = 100; // MCP registry server.json limit (bytes, to be safe)
const GITHUB_DESCRIPTION_LIMIT = 350;
const GITHUB_TOPIC_LIMIT = 20;

type Product = {
  name: string;
  tagline: string;
  description: string;
  shortDescription: string;
  formats: string[];
  formerly: string;
  keywords: string[];
  alsoFrom: Array<{ name: string; url: string; description: string }>;
};

const read = (path: string) => readFileSync(join(root, path), 'utf8');

export const product = JSON.parse(read('product.json')) as Product;

/** The description without its leading "tagline: ", used where the tagline is already shown. */
export const subtitle = (p: Product): string => {
  const prefix = `${p.tagline}: `;
  if (!p.description.startsWith(prefix)) {
    throw new Error(`product.json description must start with "${prefix}"`);
  }
  const rest = p.description.slice(prefix.length);
  return rest.charAt(0).toUpperCase() + rest.slice(1);
};

// ---------- copy ----------

const alsoFromList = (p: Product) =>
  p.alsoFrom.map((x) => `- [**${x.name}**](${x.url}): ${x.description}`).join('\n');

const readmeFormerly = (p: Product) =>
  `<sub>Formerly **${p.formerly}**. [Migrating from ${p.formerly}](https://sylphxai.github.io/anymd/guide/migration)</sub>`;

const docsFormerly = (p: Product) =>
  `<p class="cit-fine" style="text-align:center">Formerly <strong>${p.formerly}</strong>. See <a href="./guide/migration">Migration</a>.</p>`;

const docsHero = (p: Product) =>
  [`  text: ${JSON.stringify(p.tagline)}`, `  tagline: ${JSON.stringify(subtitle(p))}`].join('\n');

// ---------- regions ----------

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export const fillRegion = (text: string, name: string, body: string, yaml = false): string => {
  const [open, close] = yaml
    ? [`# generated:${name}`, `# /generated:${name}`]
    : [`<!-- generated:${name} -->`, `<!-- /generated:${name} -->`];
  const re = new RegExp(`([ \\t]*${escape(open)}\\n)[\\s\\S]*?\\n([ \\t]*${escape(close)})`);
  if (!re.test(text)) throw new Error(`missing region ${open}`);
  return text.replace(re, (_m, a: string, b: string) => `${a}${body}\n${b}`);
};

type Target = { path: string; render: (current: string) => string };

const jsonTarget = (path: string, apply: (json: Record<string, unknown>) => void): Target => ({
  path,
  render: (current) => {
    const json = JSON.parse(current) as Record<string, unknown>;
    apply(json);
    return `${JSON.stringify(json, null, 2)}\n`;
  },
});

export const targets = (p: Product): Target[] => [
  {
    path: 'README.md',
    render: (t) =>
      [
        ['lead', subtitle(p)],
        ['formerly', readmeFormerly(p)],
        ['also-from', alsoFromList(p)],
      ].reduce((acc, [n, body]) => fillRegion(acc, n as string, body as string), t),
  },
  {
    path: 'docs/index.md',
    render: (t) => {
      let out = fillRegion(t, 'hero', docsHero(p), true);
      out = fillRegion(out, 'formerly', docsFormerly(p));
      return fillRegion(out, 'also-from', alsoFromList(p));
    },
  },
  jsonTarget('package.json', (j) => {
    j['description'] = p.description;
    j['keywords'] = [p.name, ...p.keywords];
  }),
  jsonTarget('server.json', (j) => {
    j['description'] = p.shortDescription;
  }),
];

export const validate = (p: Product): string[] => {
  const errors: string[] = [];
  const bytes = Buffer.byteLength(p.shortDescription);
  if (bytes > REGISTRY_DESCRIPTION_LIMIT) {
    errors.push(`shortDescription is ${bytes} bytes; the MCP registry allows ${REGISTRY_DESCRIPTION_LIMIT}`);
  }
  if (p.description.length > GITHUB_DESCRIPTION_LIMIT) {
    errors.push(`description is ${p.description.length} chars; GitHub allows ${GITHUB_DESCRIPTION_LIMIT}`);
  }
  if (p.keywords.length > GITHUB_TOPIC_LIMIT) {
    errors.push(`${p.keywords.length} keywords; GitHub allows ${GITHUB_TOPIC_LIMIT} topics`);
  }
  for (const k of p.keywords) {
    if (!/^[a-z0-9][a-z0-9-]{0,49}$/.test(k)) errors.push(`keyword "${k}" is not a valid GitHub topic`);
  }
  for (const f of p.formats) {
    if (!p.description.includes(f)) errors.push(`description does not mention format "${f}"`);
  }
  subtitle(p);
  return errors;
};

const githubCommands = (p: Product): string[][] => [
  ['gh', 'api', '-X', 'PATCH', `repos/${REPO}`, '-f', `description=${p.description}`],
  ['gh', 'api', '-X', 'PUT', `repos/${REPO}/topics`, ...p.keywords.flatMap((k) => ['-f', `names[]=${k}`])],
];

const shellQuote = (s: string) => (/^[\w@%+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`);

if (import.meta.main) {
  const errors = validate(product);
  if (errors.length) {
    console.error(errors.map((e) => `[render-copy] ${e}`).join('\n'));
    process.exit(1);
  }
  if (process.argv.includes('--github') || process.argv.includes('--apply-github')) {
    for (const cmd of githubCommands(product)) {
      console.log(cmd.map(shellQuote).join(' '));
      if (process.argv.includes('--apply-github')) {
        const r = spawnSync(cmd[0] as string, cmd.slice(1), { stdio: ['ignore', 'ignore', 'inherit'] });
        if (r.status !== 0) process.exit(r.status ?? 1);
      }
    }
    process.exit(0);
  }
  const check = process.argv.includes('--check');
  const drift: string[] = [];
  for (const target of targets(product)) {
    const current = read(target.path);
    const next = target.render(current);
    if (next === current) continue;
    if (check) drift.push(target.path);
    else writeFileSync(join(root, target.path), next);
  }
  if (drift.length) {
    console.error(
      `[render-copy] out of date with product.json: ${drift.join(', ')}\n` +
        '[render-copy] run `bun scripts/render-copy.ts` and commit the result'
    );
    process.exit(1);
  }
  console.log(`[render-copy] ${check ? 'PASS' : 'rendered'}`);
}
