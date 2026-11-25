import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseInline, parseMarkdown } from './studio-markdown';

test('reads scalar metadata, mixed provenance, headings and wrapped paragraphs', () => {
  assert.deepEqual(parseMarkdown([
    '---', 'source: résumé.pdf', 'title: "Field Notes: 水質"', 'pages: 2', "author: 'O''Brien'", '---', '',
    '<!-- page 1 -->', '# Title', 'A line', 'wrapped around.', '## Findings', '<!-- slide 2 -->',
    '### Results', '<!-- sheet Q4 水質 -->', '#### End ####', '',
  ].join('\r\n')), {
    metadata: [{ key: 'source', value: 'résumé.pdf' }, { key: 'title', value: 'Field Notes: 水質' }, { key: 'pages', value: '2' }, { key: 'author', value: "O'Brien" }],
    blocks: [
      { kind: 'marker', label: 'page 1' }, { kind: 'heading', level: 1, text: 'Title' },
      { kind: 'paragraph', text: 'A line wrapped around.' }, { kind: 'heading', level: 2, text: 'Findings' },
      { kind: 'marker', label: 'slide 2' }, { kind: 'heading', level: 3, text: 'Results' },
      { kind: 'marker', label: 'sheet Q4 水質' }, { kind: 'heading', level: 4, text: 'End' },
    ],
  });
  assert.deepEqual(parseMarkdown('---\ntitle: not closed\n# Heading').metadata, []);
});

test('parses lists, blockquotes and both fence styles without interpreting code', () => {
  assert.deepEqual(parseMarkdown([
    '- One', '+ Two', '3. Third', '4) Fourth', '> First', '> second', '', '````ts',
    '  # code', '```', '<!-- page 7 -->', '````', '~~~', '**literal**', '~~~',
  ].join('\n')).blocks, [
    { kind: 'list', start: null, items: ['One', 'Two'] }, { kind: 'list', start: 3, items: ['Third', 'Fourth'] },
    { kind: 'quote', text: 'First\nsecond' }, { kind: 'code', text: '  # code\n```\n<!-- page 7 -->' },
    { kind: 'code', text: '**literal**' },
  ]);
  assert.deepEqual(parseMarkdown('~~~\nclosed by EOF').blocks, [{ kind: 'code', text: 'closed by EOF' }]);
});

test('handles compact converter tables, escaped pipes and GFM alignment', () => {
  assert.deepEqual(parseMarkdown('|Site||Samples|\n|-|:-:|-:|\n|A\\|B|x|14|\n|B||18|\n\nNext').blocks, [
    { kind: 'table', header: ['Site', '', 'Samples'], align: ['left', 'center', 'right'], rows: [['A|B', 'x', '14'], ['B', '', '18']] },
    { kind: 'paragraph', text: 'Next' },
  ]);
  assert.deepEqual(parseMarkdown('Site | Samples\n--- | ---\nA | 14').blocks, [
    { kind: 'table', header: ['Site', 'Samples'], align: ['left', 'left'], rows: [['A', '14']] },
  ]);
  assert.deepEqual(parseMarkdown('Not | a table\nNo delimiter').blocks, [{ kind: 'paragraph', text: 'Not | a table No delimiter' }]);
});

test('recognizes only safe inline formatting; external images, links and HTML are literal text', () => {
  const literal = '<b>Text</b> [link](https://example.invalid) ![image](https://example.invalid/image.png)';
  assert.deepEqual(parseInline(literal), [{ kind: 'text', text: literal }]);
  assert.deepEqual(parseInline('**Bold** and `**raw**` then \\*'), [
    { kind: 'strong', text: 'Bold' }, { kind: 'text', text: ' and ' }, { kind: 'code', text: '**raw**' },
    { kind: 'text', text: ' then ' }, { kind: 'text', text: '*' },
  ]);
});
