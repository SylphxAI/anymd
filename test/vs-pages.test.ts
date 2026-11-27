import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const tools = [
  'docling',
  'kreuzberg',
  'llamaparse',
  'marker',
  'markitdown',
  'pdftotext',
  'unstructured',
];

test('published comparison corpus and index retain commercial and benchmark references', () => {
  const hub = read('../docs/vs/index.md');
  const index = read('../apps/web/public/llms.txt');
  for (const tool of tools) {
    expect(hub).toContain(`(/vs/${tool})`);
    expect(index).toContain(`(https://anymd.sylphx.com/vs/${tool})`);
    const copy = read(`../docs/vs/${tool}.md`);
    expect(copy).toContain(`/pricing?entry_surface=seo_vs-${tool}`);
    if (['docling', 'llamaparse', 'markitdown'].includes(tool)) {
      expect(copy).toContain(`**[See the recorded PDF quote check](/studio?sample=pdf&entry_surface=seo_vs-${tool}#studio-cite-heading)**`);
    }
    expect(copy).toContain('US$29');
    expect(copy).toMatch(/^description: ".{120,160}"$/m);
    expect(copy).not.toMatch(/Keel|Spiron|AnyAgents|Sylphx Money/);
  }
});
