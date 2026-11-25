// A presentation subset for converter output, not an HTML or URL parser.
export type Inline = { kind: 'text' | 'strong' | 'code'; text: string };
export type Block =
  | { kind: 'marker'; label: string }
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'paragraph' | 'quote' | 'code'; text: string }
  | { kind: 'list'; start: number | null; items: string[] }
  | { kind: 'table'; header: string[]; rows: string[][]; align: ('left' | 'center' | 'right')[] };

export function parseInline(text: string): Inline[] {
  const spans: Inline[] = [];
  let at = 0;
  for (const match of text.matchAll(/\\([\\`*])|(?<!`)`([^`\n]+)`(?!`)|(?<!\*)\*\*([^*\n]+)\*\*(?!\*)/g)) {
    if (at < match.index) spans.push({ kind: 'text', text: text.slice(at, match.index) });
    const [, literal, code, strong] = match;
    spans.push(code !== undefined ? { kind: 'code', text: code } : strong !== undefined
      ? { kind: 'strong', text: strong } : { kind: 'text', text: literal });
    at = match.index + match[0].length;
  }
  if (at < text.length) spans.push({ kind: 'text', text: text.slice(at) });
  return spans;
}

function scalar(value: string) {
  if (value.startsWith('"') && value.endsWith('"')) {
    try { return JSON.parse(value) as string; } catch { /* Keep unfamiliar YAML as text. */ }
  }
  if (value.startsWith("'") && value.endsWith("'") && value.length >= 2) {
    return value.slice(1, -1).replace(/''/g, "'");
  }
  return value;
}

function frontMatter(lines: string[]) {
  const metadata: { key: string; value: string }[] = [];
  if (lines[0]?.trim() !== '---') return { metadata, next: 0 };
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---' && metadata.length) return { metadata, next: i + 1 };
    const field = /^([\w-]+):[ \t]*(.*)$/.exec(lines[i]);
    if (!field) break;
    metadata.push({ key: field[1], value: scalar(field[2].trim()) });
  }
  return { metadata: [], next: 0 };
}

function tableCells(input: string) {
  const line = input.trim();
  const cells: string[] = [];
  let start = 0;
  let cell = '';
  let hasPipe = false;
  for (const match of line.matchAll(/\\[\\|]|\|/g)) {
    if (match[0] === '|') {
      hasPipe = true;
      if (match.index > 0) cells.push((cell + line.slice(start, match.index)).trim());
      cell = '';
    } else cell += line.slice(start, match.index) + match[0].slice(1);
    start = match.index + match[0].length;
  }
  if (start < line.length || cell) cells.push((cell + line.slice(start)).trim());
  return hasPipe ? cells : [];
}

const listItem = /^(?:([-+*])|(\d{1,9})[.)])[ \t]+(.*)$/;
const quoteLine = /^> ?(.*)$/;
const fenceLine = /^ {0,3}(`{3,}|~{3,})/;

export function parseMarkdown(markdown: string) {
  const lines = markdown.replace(/^\uFEFF/, '').split(/\r\n?|\n/);
  const { metadata, next } = frontMatter(lines);
  const blocks: Block[] = [];
  let i = next;
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
    paragraph = [];
  };
  while (i < lines.length) {
    const raw = lines[i++];
    const line = raw.trim();
    if (!line) { flush(); continue; }
    const marker = /^<!--\s*((?:page|slide|chapter) \d+(?:: .+)?|sheet .+?)\s*-->$/.exec(line);
    if (marker) { flush(); blocks.push({ kind: 'marker', label: marker[1] }); continue; }
    const fence = fenceLine.exec(raw);
    if (fence) {
      flush();
      const closes = (candidate: string) => {
        const trimmed = candidate.trimEnd();
        const end = fenceLine.exec(trimmed);
        return end?.[1][0] === fence[1][0] && end[1].length >= fence[1].length && end[0] === trimmed;
      };
      const start = i;
      while (i < lines.length && !closes(lines[i])) i++;
      blocks.push({ kind: 'code', text: lines.slice(start, i).join('\n') });
      i++;
      continue;
    }
    const heading = /^(#{1,4})(?:[ 	]+(.*)|$)/.exec(line);
    if (heading) {
      flush();
      blocks.push({ kind: 'heading', level: heading[1].length, text: (heading[2] ?? '').replace(/\s+#+$/, '') });
      continue;
    }
    const item = listItem.exec(line);
    if (item) {
      flush();
      const start = item[2] === undefined ? null : Number(item[2]);
      const items = [item[3]];
      let rest: RegExpExecArray | null;
      while ((rest = listItem.exec(lines[i]?.trim() ?? '')) && (rest[2] === undefined) === (start === null)) {
        items.push(rest[3]); i++;
      }
      blocks.push({ kind: 'list', start, items });
      continue;
    }
    const quote = quoteLine.exec(line);
    if (quote) {
      flush();
      const parts = [quote[1]];
      let rest: RegExpExecArray | null;
      while ((rest = quoteLine.exec(lines[i]?.trim() ?? ''))) { parts.push(rest[1]); i++; }
      blocks.push({ kind: 'quote', text: parts.join('\n') });
      continue;
    }
    const header = line.includes('|') ? tableCells(line) : [];
    const divider = header.length ? tableCells(lines[i] ?? '') : [];
    if (header.length && divider.length === header.length && divider.every(cell => /^:?-+:?$/.test(cell))) {
      flush(); i++;
      const rows: string[][] = [];
      while (i < lines.length) {
        const cells = tableCells(lines[i]);
        if (!cells.length) break;
        rows.push(cells.slice(0, header.length));
        i++;
      }
      const align = divider.map(cell => cell.endsWith(':') ? (cell.startsWith(':') ? 'center' : 'right') : 'left');
      blocks.push({ kind: 'table', header, rows, align });
      continue;
    }
    paragraph.push(line);
  }
  flush();
  return { metadata, blocks };
}
