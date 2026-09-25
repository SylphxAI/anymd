#!/usr/bin/env bun
/**
 * Render public copy from one source: product.json (names, descriptions, keywords,
 * sibling projects) and the committed benchmark results it points at.
 *
 * Generated regions are fenced in Markdown as
 *   <!-- generated:NAME --> … <!-- /generated:NAME -->
 * and in YAML front matter as
 *   # generated:NAME … # /generated:NAME
 * Edit product.json or the results JSON, never the fenced text.
 *
 * Usage:
 *   bun scripts/render-copy.ts                    # rewrite every target
 *   bun scripts/render-copy.ts --check            # fail if any target drifted
 *   bun scripts/render-copy.ts --results FILE     # use another benchmark results JSON
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
  benchmark: {
    results: string;
    tools: Record<string, string>;
    pdfOnly: string[];
    sample: { doc: string; name: string };
    web: { doc: string; name: string };
  };
  alsoFrom: Array<{ name: string; url: string; description: string }>;
};

type Row = {
  doc: string;
  kind: string;
  tool: string;
  seconds: number;
  tokens: number;
  text_ok?: number;
  text_total?: number;
  order_ok?: boolean | null;
  table_rows?: number;
  table_total?: number;
  error?: string;
};

type Results = { meta: { date: string; machine: string; cpus: number; runs: number }; results: Row[] };

const read = (path: string) => readFileSync(join(root, path), 'utf8');
const argValue = (flag: string) => {
  const i = process.argv.indexOf(flag);
  return i >= 0 ? process.argv[i + 1] : undefined;
};

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

// ---------- benchmark numbers ----------

const secs = (s: number) => `${s < 1 ? s.toFixed(2) : s < 100 ? s.toFixed(1) : Math.round(s)} s`;
const kTok = (t: number) => `${(t / 1000).toFixed(1)}k`;
const times = (a: number, b: number) => `${Math.round(a / b)}×`;
const unique = <T>(xs: T[]) => [...new Set(xs)];

class Bench {
  readonly rows: Row[];
  readonly tools: string[];
  readonly docs: string[];
  readonly pdfDocs: Set<string>;

  constructor(
    readonly data: Results,
    readonly cfg: Product['benchmark']
  ) {
    this.rows = data.results;
    this.tools = unique(this.rows.map((r) => r.tool));
    this.docs = unique(this.rows.map((r) => r.doc));
    const pdfTool = cfg.pdfOnly[0];
    this.pdfDocs = new Set(
      this.rows.filter((r) => r.tool === pdfTool || r.kind.startsWith('pdf')).map((r) => r.doc)
    );
  }

  name(tool: string) {
    return this.cfg.tools[tool] ?? tool;
  }

  row(doc: string, tool: string): Row {
    const r = this.rows.find((x) => x.doc === doc && x.tool === tool && !x.error);
    if (!r) throw new Error(`benchmark results have no ${tool} row for ${doc}`);
    return r;
  }

  /** Totals the way bench/report.py computes them: quality checks count PDF documents only. */
  totals(tool: string) {
    const ok = this.rows.filter((r) => r.tool === tool && !r.error);
    const q = this.cfg.pdfOnly.includes(tool) ? ok : ok.filter((r) => this.pdfDocs.has(r.doc));
    const sum = (rs: Row[], f: (r: Row) => number) => rs.reduce((n, r) => n + f(r), 0);
    const order = q.filter((r) => r.order_ok !== undefined && r.order_ok !== null);
    return {
      seconds: sum(ok, (r) => r.seconds),
      tokens: sum(ok, (r) => r.tokens),
      textOk: sum(q, (r) => r.text_ok ?? 0),
      textTotal: sum(q, (r) => r.text_total ?? 0),
      tableOk: sum(q, (r) => r.table_rows ?? 0),
      tableTotal: sum(q, (r) => r.table_total ?? 0),
      orderOk: order.filter((r) => r.order_ok).length,
      orderTotal: order.length,
      errors: this.rows.filter((r) => r.tool === tool && r.error).length,
    };
  }

  corpusLine() {
    const pdfs = this.docs.filter((d) => this.pdfDocs.has(d)).length;
    const others = this.docs
      .filter((d) => !this.pdfDocs.has(d))
      .map((d) => this.rows.find((r) => r.doc === d)?.kind.split(':')[0]?.toUpperCase() ?? d);
    const rest = others.length > 1 ? `${others.slice(0, -1).join(', ')}, and ${others.at(-1)}` : others.join('');
    return `${this.docs.length} real documents (${pdfs} PDFs, plus ${rest})`;
  }

  runLine() {
    const m = this.data.meta;
    return `Benchmark run ${m.date} on ${m.cpus} CPUs (${m.machine}), median of ${m.runs} runs (docling: 1).`;
  }
}

const loadBench = (p: Product) => {
  const path = argValue('--results') ?? p.benchmark.results;
  return new Bench(JSON.parse(read(path)) as Results, p.benchmark);
};

const sampleName = (b: Bench) => b.cfg.sample.name;
const plain = (md: string) => md.replace(/\*/g, '');

export const fastBullet = (b: Bench) => {
  const s = b.cfg.sample.doc;
  const a = b.row(s, 'anymd');
  const tables = a.table_total && a.table_rows === a.table_total ? ', with every table intact' : '';
  return `- **Fast.** Native Rust converts in parallel, page by page. On ${sampleName(b)}, anymd takes **${secs(a.seconds)}**: ${times(b.row(s, 'markitdown').seconds, a.seconds)} faster than MarkItDown and ${times(b.row(s, 'docling').seconds, a.seconds)} faster than docling${tables}.`;
};

export const fastFeature = (b: Bench) => {
  const s = b.cfg.sample.doc;
  const name = plain(sampleName(b));
  const detail = `Native Rust converts in parallel, page by page. ${name.charAt(0).toUpperCase()}${name.slice(1)} takes ${secs(b.row(s, 'anymd').seconds)}; MarkItDown needs ${secs(b.row(s, 'markitdown').seconds)}.`;
  return `    details: ${JSON.stringify(detail)}`;
};

export const benchSummary = (b: Bench) => {
  const order = ['anymd', ...Object.keys(b.cfg.tools).filter((t) => t !== 'anymd' && b.tools.includes(t))];
  const t = Object.fromEntries(order.map((tool) => [tool, b.totals(tool)]));
  const a = b.totals('anymd');
  const mark = (tool: string) => (b.cfg.pdfOnly.includes(tool) ? ' ¹' : '');
  const cell = (tool: string, v: string) => (tool === 'anymd' ? `**${v}**` : v);
  const line = (label: string, f: (tool: string) => string) =>
    `| ${label} | ${order.map((tool) => cell(tool, f(tool))).join(' | ')} |`;
  const pdfOnlyNames = b.cfg.pdfOnly.map((x) => b.name(x)).join(', ');

  const s = b.cfg.sample.doc;
  const w = b.cfg.web.doc;
  const others = order.filter((x) => x !== 'anymd');
  const sampleTimes = ['docling', 'markitdown']
    .map((x) => `${b.name(x)} ${secs(b.row(s, x).seconds)}`)
    .join(', and ');
  const md = b.row(s, 'markitdown');
  const webOthers = others
    .filter((x) => !b.cfg.pdfOnly.includes(x))
    .map((x) => ({ x, tok: b.row(w, x).tokens }))
    .sort((p, q) => p.tok - q.tok)
    .map(({ x, tok }) => `${b.name(x)} ${kTok(tok)}`);

  return [
    `${b.corpusLine()}, on ${b.data.meta.cpus} CPUs (${b.data.meta.machine}), ${b.data.meta.date}:`,
    '',
    `| | ${order.map((x) => (x === 'anymd' ? '**anymd**' : b.name(x))).join(' | ')} |`,
    `|---|${order.map(() => '---').join('|')}|`,
    line(`Total time, ${b.docs.length} documents`, (x) => `${secs(t[x]?.seconds ?? 0)}${mark(x)}`),
    line(`Sentences intact (${a.textTotal})`, (x) => `${t[x]?.textOk}`),
    line(`Table rows recovered (${a.tableTotal})`, (x) => `${t[x]?.tableOk}`),
    line(`Reading order correct (${a.orderTotal})`, (x) => `${t[x]?.orderOk}`),
    line('Output tokens (o200k)', (x) => `${kTok(t[x]?.tokens ?? 0)}${mark(x)}`),
    '',
    `<sub>¹ ${pdfOnlyNames} reads PDFs only and outputs plain text without tables.</sub>`,
    '',
    `On ${sampleName(b)}, anymd takes **${secs(b.row(s, 'anymd').seconds)}**, ${sampleTimes}; MarkItDown keeps ${md.text_ok ?? 0} of ${md.text_total ?? 0} reference sentences intact. On ${b.cfg.web.name}, anymd's main-content extraction uses **${kTok(b.row(w, 'anymd').tokens)} tokens**; ${webOthers.join(', ')}.`,
  ].join('\n');
};

export const benchTables = (b: Bench) => {
  const lines = [b.runLine(), '', `| document | ${b.tools.join(' | ')} |`, `|---|${'---|'.repeat(b.tools.length)}`];
  for (const doc of b.docs) {
    const cells = b.tools.map((tool) => {
      const r = b.rows.find((x) => x.doc === doc && x.tool === tool);
      if (!r) return 'n/a';
      if (r.error) return 'error';
      const parts = [`${r.seconds.toFixed(2)}s`, `${r.tokens.toLocaleString('en-US')} tok`];
      if (r.text_total) parts.push(`text ${r.text_ok}/${r.text_total}`);
      if (r.table_total) parts.push(`tables ${r.table_rows}/${r.table_total}`);
      return parts.join(' · ');
    });
    lines.push(`| ${doc} | ${cells.join(' | ')} |`);
  }
  lines.push(
    '',
    '| tool | total time (s) | total tokens | sentences intact | table rows recovered | reading order ok |',
    '|---|---|---|---|---|---|'
  );
  for (const tool of b.tools) {
    const t = b.totals(tool);
    const note = t.errors ? ` (${t.errors} errors)` : '';
    lines.push(
      `| ${tool}${note} | ${t.seconds.toFixed(2)} | ${t.tokens.toLocaleString('en-US')} | ${t.textOk}/${t.textTotal} | ${t.tableOk}/${t.tableTotal} | ${t.orderOk}/${t.orderTotal} |`
    );
  }
  return lines.join('\n');
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

export const targets = (p: Product, b: Bench): Target[] => [
  {
    path: 'README.md',
    render: (t) =>
      [
        ['lead', subtitle(p)],
        ['formerly', readmeFormerly(p)],
        ['bench-fast', fastBullet(b)],
        ['bench-summary', benchSummary(b)],
        ['also-from', alsoFromList(p)],
      ].reduce((acc, [n, body]) => fillRegion(acc, n as string, body as string), t),
  },
  {
    path: 'docs/index.md',
    render: (t) => {
      let out = fillRegion(t, 'hero', docsHero(p), true);
      out = fillRegion(out, 'bench-fast', fastFeature(b), true);
      out = fillRegion(out, 'formerly', docsFormerly(p));
      return fillRegion(out, 'also-from', alsoFromList(p));
    },
  },
  { path: 'docs/guide/benchmarks.md', render: (t) => fillRegion(t, 'bench-tables', benchTables(b)) },
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
  const bench = loadBench(product);
  const drift: string[] = [];
  for (const target of targets(product, bench)) {
    const current = read(target.path);
    const next = target.render(current);
    if (next === current) continue;
    if (check) drift.push(target.path);
    else writeFileSync(join(root, target.path), next);
  }
  if (drift.length) {
    console.error(
      `[render-copy] out of date with product.json / benchmark results: ${drift.join(', ')}\n` +
        '[render-copy] run `bun scripts/render-copy.ts` and commit the result'
    );
    process.exit(1);
  }
  console.log(`[render-copy] ${check ? 'PASS' : 'rendered'}`);
}
