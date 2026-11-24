import { Braces, Check, Clapperboard, Copy, Files, FileText } from 'lucide-solid';
import { createSignal, createUniqueId, For, onCleanup, Show } from 'solid-js';
import './home-conversion-demo.css';

type Locale = 'en' | 'zh-TW';
const examples = ['pdf', 'document', 'video'] as const;
type Example = (typeof examples)[number];

type DemoCopy = {
  title: string;
  preview: string;
  output: string;
  copy: string;
  copied: string;
  copyFailed: string;
  notice: string;
  nativePro: string;
  examples: Record<Example, { tab: string; file: string; description: string; markdown: string }>;
};

// Authored examples, not live output. Page and slide anchors mirror native read;
// the frame citation summarizes the timestamps used in docs/tasks/extract-video-frame.md.
const copy: Record<Locale, DemoCopy> = {
  en: {
    title: 'Conversion examples',
    preview: 'Illustrative preview',
    output: 'Markdown output',
    copy: 'Copy Markdown',
    copied: 'Markdown copied.',
    copyFailed: 'Couldn’t copy. Select the example text to copy it manually.',
    notice: 'This preview does not convert files.',
    nativePro: 'Pro · Native CLI / MCP',
    examples: {
      pdf: {
        tab: 'PDF',
        file: 'water-survey.pdf',
        description: 'Page markers and a table, kept in reading order.',
        markdown: `<!-- page 3 -->

## River survey

|Site|Turbidity (NTU)|
|-|-|
|North bank|4.2|
|South bank|2.8|`,
      },
      document: {
        tab: 'Multi-page document',
        file: 'field-brief.pptx',
        description: 'Slide headings and lists stay in source order.',
        markdown: `<!-- slide 1 -->

## Field brief
- Sample both banks after the rain.

<!-- slide 2 -->

## Next visit
- Recheck the south bank in two weeks.`,
      },
      video: {
        tab: 'Video frame',
        file: 'demo.mkv',
        description: 'A Markdown citation based on exact frame metadata.',
        markdown: `## Frame citation

- Source: demo.mkv
- Requested: 00:01.031
- Decoded: 00:01.100
- Image: 320 × 240 PNG
- PNG SHA-256: omitted in this preview`,
      },
    },
  },
  'zh-TW': {
    title: '轉換範例',
    preview: '示意預覽',
    output: 'Markdown 輸出',
    copy: '複製 Markdown',
    copied: '已複製 Markdown。',
    copyFailed: '無法複製，請選取範例文字後手動複製。',
    notice: '此預覽不會轉換檔案。',
    nativePro: 'Pro · 本機 CLI／MCP',
    examples: {
      pdf: {
        tab: 'PDF',
        file: '水質調查.pdf',
        description: '依閱讀順序整理內容，保留頁碼標記與表格。',
        markdown: `<!-- page 3 -->

## 河川水質調查

|採樣地點|濁度（NTU）|
|-|-|
|北岸|4.2|
|南岸|2.8|`,
      },
      document: {
        tab: '多頁文件',
        file: '採樣簡報.pptx',
        description: '依原始投影片的順序保留標題和清單。',
        markdown: `<!-- slide 1 -->

## 採樣計畫
- 雨後到兩岸採集水樣。

<!-- slide 2 -->

## 下次勘查
- 兩週後回到南岸複測。`,
      },
      video: {
        tab: '影片影格',
        file: 'demo.mkv',
        description: '用精確影格的資訊撰寫 Markdown 引用。',
        markdown: `## 影格引用

- 來源：demo.mkv
- 指定時間：00:01.031
- 實際解碼時間：00:01.100
- 影像：320 × 240 PNG
- PNG 的 SHA-256：此預覽省略`,
      },
    },
  },
};

const icons = { pdf: FileText, document: Files, video: Clapperboard };

export function HomeConversionDemo(props: { locale: Locale }) {
  const labels = () => copy[props.locale];
  const id = createUniqueId();
  const [active, setActive] = createSignal<Example>('pdf');
  const [feedback, setFeedback] = createSignal<'copied' | 'copyFailed'>();
  const tabButtons: Partial<Record<Example, HTMLButtonElement>> = {};
  let copyAttempt = 0;

  const tabId = (key: Example) => id + '-tab-' + key;
  const panelId = (key: Example) => id + '-panel-' + key;
  const titleId = id + '-title';

  onCleanup(() => { copyAttempt++; });

  function selectExample(key: Example) {
    if (key === active()) return;
    copyAttempt++;
    setFeedback(undefined);
    setActive(key);
  }

  function navigateTabs(event: KeyboardEvent, key: Example) {
    const index = examples.indexOf(key);
    let next: Example;
    switch (event.key) {
      case 'ArrowLeft': next = examples[(index + examples.length - 1) % examples.length]; break;
      case 'ArrowRight': next = examples[(index + 1) % examples.length]; break;
      case 'Home': next = examples[0]; break;
      case 'End': next = examples[examples.length - 1]; break;
      default: return;
    }
    event.preventDefault();
    selectExample(next);
    tabButtons[next]?.focus();
  }

  async function copyMarkdown(markdown: string) {
    const attempt = ++copyAttempt;
    setFeedback(undefined);
    let result: 'copied' | 'copyFailed';
    try {
      await navigator.clipboard.writeText(markdown);
      result = 'copied';
    } catch {
      result = 'copyFailed';
    }
    if (attempt === copyAttempt) setFeedback(result);
  }

  return (
    <section class="home-conversion-demo" aria-labelledby={titleId} lang={props.locale}>
      <div class="home-conversion-demo-header">
        <span class="home-conversion-demo-mark"><Braces size={18} aria-hidden="true" /></span>
        <h2 id={titleId}>{labels().title}</h2>
        <span class="home-conversion-demo-preview">{labels().preview}</span>
      </div>
      <div class="home-conversion-demo-tabs" role="tablist" aria-labelledby={titleId}>
        <For each={examples}>{(key) => {
          const Icon = icons[key];
          return (
            <button
              type="button"
              role="tab"
              id={tabId(key)}
              aria-selected={active() === key}
              aria-controls={panelId(key)}
              tabIndex={active() === key ? 0 : -1}
              ref={(button) => { tabButtons[key] = button; }}
              onClick={() => selectExample(key)}
              onKeyDown={(event) => navigateTabs(event, key)}
            >
              <Icon size={15} aria-hidden="true" />
              <span>{labels().examples[key].tab}</span>
            </button>
          );
        }}</For>
      </div>
      <For each={examples}>{(key) => {
        const example = () => labels().examples[key];
        return (
          <div
            class="home-conversion-demo-panel"
            role="tabpanel"
            id={panelId(key)}
            aria-labelledby={tabId(key)}
            tabIndex={0}
            hidden={active() !== key}
          >
            <div class="home-conversion-demo-source">
              <div>
                <code>{example().file}</code>
                <Show when={key === 'video'}><span class="home-conversion-demo-pro">{labels().nativePro}</span></Show>
              </div>
              <p>{example().description}</p>
            </div>
            <div class="home-conversion-demo-output">
              <div class="home-conversion-demo-output-bar">
                <span>{labels().output}</span>
                <button type="button" class="home-conversion-demo-copy" onClick={() => void copyMarkdown(example().markdown)}>
                  <Show when={feedback() === 'copied'} fallback={<Copy size={14} aria-hidden="true" />}>
                    <Check size={14} aria-hidden="true" />
                  </Show>
                  {labels().copy}
                </button>
              </div>
              <pre tabIndex={0} aria-label={props.locale === 'en' && key === 'pdf' ? 'Example Markdown' : labels().output}><code><For each={example().markdown.split('\n')}>{(line, index) => (
                <span classList={{
                  'home-conversion-demo-anchor': line.startsWith('<!--'),
                  'home-conversion-demo-heading': line.startsWith('## '),
                }}>{index() > 0 ? '\n' : ''}{line}</span>
              )}</For></code></pre>
            </div>
          </div>
        );
      }}</For>
      <div class="home-conversion-demo-footer">
        <span>{labels().notice}</span>
        <span role="status">{feedback() === 'copied' ? labels().copied : feedback() === 'copyFailed' ? labels().copyFailed : ''}</span>
      </div>
    </section>
  );
}
