<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { withBase } from 'vitepress';

type Status = 'loading' | 'ready' | 'busy' | 'unavailable';

interface Result {
  name: string;
  size: number;
  markdown: string;
  format: string;
  title: string | null;
  units: number;
  unitNoun: string;
  tokens: number;
  ms: number;
}

interface Renderer {
  render: (source: string) => string;
}

const status = ref<Status>('loading');
const version = ref('');
const loadMs = ref(0);
const error = ref('');
const result = shallowRef<Result | null>(null);
const tab = ref<'rendered' | 'raw'>('rendered');
const dragging = ref(false);
const copied = ref(false);
const renderer = shallowRef<Renderer | null>(null);
const fileInput = ref<HTMLInputElement | null>(null);

let worker: Worker | null = null;
let nextId = 0;
let pending: { id: number; name: string; size: number } | null = null;

const wasmBase = () => new URL(withBase('/wasm/'), window.location.href).href;

function startWorker() {
  worker?.terminate();
  worker = new Worker(new URL('./anymd.worker.ts', import.meta.url), { type: 'module' });
  worker.onmessage = (event: MessageEvent) => {
    const data = event.data;
    if (data.type === 'ready') {
      status.value = 'ready';
      version.value = data.version;
      loadMs.value = data.loadMs;
    } else if (data.type === 'unavailable') {
      status.value = 'unavailable';
      error.value = data.error;
    } else if (pending && data.id === pending.id) {
      const { name, size } = pending;
      pending = null;
      if (data.type === 'result') {
        status.value = 'ready';
        error.value = '';
        result.value = {
          name,
          size,
          markdown: data.markdown,
          format: data.format,
          title: data.title,
          units: data.units,
          unitNoun: data.unit_noun,
          tokens: data.tokens,
          ms: data.ms,
        };
      } else {
        error.value = `${name}: ${data.error}`;
        result.value = null;
        status.value = 'loading';
        startWorker();
      }
    }
  };
  worker.onerror = () => {
    status.value = 'unavailable';
    error.value = 'The converter worker failed to start.';
  };
  worker.postMessage({ type: 'init', base: wasmBase() });
}

onMounted(async () => {
  startWorker();
  const { default: MarkdownIt } = await import('markdown-it');
  // Raw HTML stays escaped: the Markdown comes from an untrusted file.
  const md = new MarkdownIt({ html: false, linkify: true });
  // Images become labels so the preview never fetches URLs found in the file.
  md.renderer.rules.image = (tokens, index) =>
    `<span class="pg-img">[image${tokens[index].content ? `: ${md.utils.escapeHtml(tokens[index].content)}` : ''}]</span>`;
  const linkOpen = md.renderer.rules.link_open ?? ((tokens, index, options, _env, self) => self.renderToken(tokens, index, options));
  md.renderer.rules.link_open = (tokens, index, options, env, self) => {
    tokens[index].attrSet('target', '_blank');
    tokens[index].attrSet('rel', 'noopener noreferrer nofollow');
    return linkOpen(tokens, index, options, env, self);
  };
  renderer.value = md;
});

onBeforeUnmount(() => worker?.terminate());

async function convertFile(file: File) {
  if (!worker || status.value === 'unavailable') return;
  const bytes = await file.arrayBuffer();
  pending = { id: ++nextId, name: file.name, size: file.size };
  status.value = 'busy';
  error.value = '';
  worker.postMessage({ type: 'convert', id: pending.id, name: file.name, bytes }, [bytes]);
}

function onDrop(event: DragEvent) {
  dragging.value = false;
  const file = event.dataTransfer?.files?.[0];
  if (file) void convertFile(file);
}

function onPick(event: Event) {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];
  if (file) void convertFile(file);
  input.value = '';
}

async function trySample() {
  const response = await fetch(withBase('/playground/sample.pdf'));
  const blob = await response.blob();
  await convertFile(new File([blob], 'sample.pdf', { type: 'application/pdf' }));
}

const frontMatter = computed(() => {
  const markdown = result.value?.markdown ?? '';
  const match = /^---\n([\s\S]*?)\n---\n/.exec(markdown);
  if (!match) return { entries: [] as [string, string][], body: markdown };
  const entries = match[1].split('\n').map((line) => {
    const index = line.indexOf(': ');
    return [line.slice(0, index), line.slice(index + 2).replace(/^"(.*)"$/, '$1')] as [string, string];
  });
  return { entries, body: markdown.slice(match[0].length) };
});

/** Body split at `<!-- page N -->` style markers, each piece rendered as HTML. */
const pieces = computed(() => {
  const md = renderer.value;
  if (!md) return [];
  const out: { marker: string | null; html: string }[] = [];
  let marker: string | null = null;
  let buffer: string[] = [];
  const flush = () => {
    if (marker !== null || buffer.join('').trim()) out.push({ marker, html: md.render(buffer.join('\n')) });
    buffer = [];
  };
  for (const line of frontMatter.value.body.split('\n')) {
    const found = /^<!-- (.+) -->$/.exec(line);
    if (found) {
      flush();
      marker = found[1];
    } else {
      buffer.push(line);
    }
  }
  flush();
  return out;
});

async function copy() {
  if (!result.value) return;
  await navigator.clipboard.writeText(result.value.markdown);
  copied.value = true;
  setTimeout(() => (copied.value = false), 1500);
}

function download() {
  if (!result.value) return;
  const blob = new Blob([result.value.markdown], { type: 'text/markdown;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = `${result.value.name.replace(/\.[^.]+$/, '') || 'document'}.md`;
  link.click();
  URL.revokeObjectURL(url);
}

const humanSize = (bytes: number) =>
  bytes < 1024 ? `${bytes} B` : bytes < 1048576 ? `${(bytes / 1024).toFixed(0)} KB` : `${(bytes / 1048576).toFixed(1)} MB`;

const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`;
</script>

<template>
  <div class="pg">
    <div
      class="pg-drop"
      :class="{ 'is-drag': dragging, 'is-busy': status === 'busy', 'is-off': status === 'unavailable' }"
      role="button"
      tabindex="0"
      aria-label="Drop a file or choose one to convert"
      @click="fileInput?.click()"
      @keydown.enter.prevent="fileInput?.click()"
      @keydown.space.prevent="fileInput?.click()"
      @dragover.prevent="dragging = true"
      @dragleave.prevent="dragging = false"
      @drop.prevent="onDrop"
    >
      <input ref="fileInput" type="file" hidden data-testid="file" @change="onPick" />
      <template v-if="status === 'unavailable'">
        <strong>The converter is not in this build.</strong>
        <span>Build it with <code>scripts/build-wasm.sh</code>, or install the CLI: <code>npx @sylphx/anymd file.pdf</code></span>
      </template>
      <template v-else-if="status === 'loading'">
        <strong>Loading the converter…</strong>
        <span>About 1 MB, once; cached after that.</span>
      </template>
      <template v-else-if="status === 'busy'">
        <strong>Converting…</strong>
        <span>Running in a Web Worker on your device.</span>
      </template>
      <template v-else>
        <strong>Drop a file here, or click to choose one</strong>
        <span>PDF · DOCX · PPTX · XLSX/ODS · CSV/TSV · EPUB · HTML · Markdown/text · SRT/VTT · images and media (metadata)</span>
      </template>
    </div>

    <div class="pg-bar">
      <button class="pg-btn" :disabled="status !== 'ready'" data-testid="sample" @click="trySample">Try a sample</button>
      <span class="pg-privacy"><b>Private:</b> files never leave your browser. No upload, no server.</span>
      <span v-if="version" class="pg-meta">anymd {{ version }} · WebAssembly</span>
    </div>

    <p v-if="error" class="pg-error" data-testid="error">{{ error }}</p>

    <section v-if="result" class="pg-out" data-testid="result">
      <div class="pg-stats">
        <span><b>{{ result.name }}</b> · {{ humanSize(result.size) }}</span>
        <span>{{ result.format }}<template v-if="result.units > 1"> · {{ plural(result.units, result.unitNoun) }}</template></span>
        <span data-testid="tokens">≈ {{ result.tokens.toLocaleString() }} tokens</span>
        <span data-testid="elapsed">{{ Math.max(1, Math.round(result.ms)) }} ms</span>
      </div>
      <div class="pg-tabs" role="tablist">
        <button role="tab" data-testid="tab-rendered" :aria-selected="tab === 'rendered'" :class="{ on: tab === 'rendered' }" @click="tab = 'rendered'">Rendered</button>
        <button role="tab" data-testid="tab-raw" :aria-selected="tab === 'raw'" :class="{ on: tab === 'raw' }" @click="tab = 'raw'">Markdown</button>
        <span class="pg-spacer" />
        <button class="pg-btn" @click="copy">{{ copied ? 'Copied' : 'Copy' }}</button>
        <button class="pg-btn" @click="download">Download .md</button>
      </div>
      <div v-if="tab === 'rendered'" class="pg-rendered vp-doc">
        <table v-if="frontMatter.entries.length" class="pg-front">
          <tr v-for="[key, value] in frontMatter.entries" :key="key">
            <th>{{ key }}</th>
            <td>{{ value }}</td>
          </tr>
        </table>
        <template v-for="(piece, index) in pieces" :key="index">
          <div v-if="piece.marker" class="pg-marker">{{ piece.marker }}</div>
          <div v-html="piece.html" />
        </template>
      </div>
      <pre v-else class="pg-raw" data-testid="raw">{{ result.markdown }}</pre>
    </section>
  </div>
</template>

<style scoped>
.pg { margin-top: 24px; }
.pg-drop {
  display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px;
  min-height: 170px; padding: 24px; text-align: center; cursor: pointer;
  border: 2px dashed var(--vp-c-divider); border-radius: var(--cit-radius, 14px);
  background: var(--vp-c-bg-soft); transition: border-color .15s, background .15s;
}
.pg-drop:hover, .pg-drop:focus-visible, .pg-drop.is-drag { border-color: var(--vp-c-brand-1); background: var(--vp-c-brand-soft); outline: none; }
.pg-drop.is-busy { cursor: progress; }
.pg-drop.is-off { cursor: default; }
.pg-drop strong { font-size: 1.05rem; color: var(--vp-c-text-1); }
.pg-drop span { font-size: .85rem; color: var(--vp-c-text-2); }
.pg-bar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin: 14px 0; }
.pg-privacy { font-size: .85rem; color: var(--vp-c-text-2); }
.pg-meta { margin-left: auto; font-size: .8rem; color: var(--vp-c-text-3); }
.pg-btn {
  white-space: nowrap;
  padding: 5px 14px; border-radius: 999px; font-size: .85rem; font-weight: 600;
  border: 1px solid var(--vp-c-brand-1); color: var(--vp-c-brand-1); background: transparent;
}
.pg-btn:hover:not(:disabled) { background: var(--vp-c-brand-soft); }
.pg-btn:disabled { opacity: .45; cursor: not-allowed; }
.pg-error { color: var(--vp-c-danger-1); font-size: .9rem; }
.pg-out { border: 1px solid var(--vp-c-divider); border-radius: var(--cit-radius, 14px); overflow: hidden; }
.pg-stats {
  display: flex; flex-wrap: wrap; gap: 6px 18px; padding: 10px 16px; font-size: .85rem;
  color: var(--vp-c-text-2); background: var(--vp-c-bg-soft); border-bottom: 1px solid var(--vp-c-divider);
}
.pg-tabs { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--vp-c-divider); }
.pg-tabs [role='tab'] { padding: 4px 10px; font-size: .9rem; color: var(--vp-c-text-2); border-bottom: 2px solid transparent; }
.pg-tabs [role='tab'].on { color: var(--vp-c-text-1); border-bottom-color: var(--vp-c-brand-1); }
.pg-spacer { flex: 1; }
.pg-rendered { padding: 8px 20px 20px; max-height: 70vh; overflow: auto; }
.pg-rendered :deep(.pg-img) { font-size: .8rem; color: var(--vp-c-text-3); }
.pg-front { font-size: .8rem; margin: 12px 0; }
.pg-front th { text-align: left; font-weight: 600; }
.pg-marker {
  margin: 22px 0 6px; font-size: .75rem; letter-spacing: .06em; text-transform: uppercase;
  color: var(--vp-c-text-3); border-top: 1px dashed var(--vp-c-divider); padding-top: 6px;
}
.pg-raw {
  margin: 0; padding: 16px 20px; max-height: 70vh; overflow: auto; white-space: pre-wrap; word-break: break-word;
  font-family: var(--vp-font-family-mono); font-size: .8rem; line-height: 1.55;
  background: var(--vp-code-block-bg); color: var(--vp-code-block-color);
}
</style>
