import { CampaignLink } from '@/lib/attribution-link';
import { homeDemoCopy, homeDemoExamples, type HomeDemoExample } from '@/lib/home-demo-copy';
import { localizedPath, type Locale } from '@/lib/marketing-locale';
import { ArrowRight, Braces, Check, Clapperboard, Copy, Files, FileText } from 'lucide-solid';
import { createSignal, createUniqueId, For, onCleanup, Show } from 'solid-js';
import { useCookieConsent } from './cookie-consent';
import MarkdownPreview, { MarkdownViewToggle, type MarkdownView } from './studio/markdown-preview';
import './home-conversion-demo.css';

const icons = { pdf: FileText, document: Files, video: Clapperboard };

export function HomeConversionDemo(props: { locale: Locale }) {
  const consent = useCookieConsent();
  const labels = () => homeDemoCopy[props.locale];
  const id = createUniqueId();
  const [active, setActive] = createSignal<HomeDemoExample>('pdf');
  const [view, setView] = createSignal<MarkdownView>('markdown');
  const [feedback, setFeedback] = createSignal<'copied' | 'copyFailed'>();
  const tabButtons: Partial<Record<HomeDemoExample, HTMLButtonElement>> = {};
  let copyAttempt = 0;

  const tabId = (key: HomeDemoExample) => id + '-tab-' + key;
  const panelId = (key: HomeDemoExample) => id + '-panel-' + key;
  const titleId = id + '-title';

  onCleanup(() => { copyAttempt++; });

  function selectExample(key: HomeDemoExample) {
    if (key === active()) return;
    copyAttempt++;
    setFeedback(undefined);
    setActive(key);
  }

  function navigateTabs(event: KeyboardEvent, key: HomeDemoExample) {
    const index = homeDemoExamples.indexOf(key);
    let next: HomeDemoExample;
    switch (event.key) {
      case 'ArrowLeft': next = homeDemoExamples[(index + homeDemoExamples.length - 1) % homeDemoExamples.length]; break;
      case 'ArrowRight': next = homeDemoExamples[(index + 1) % homeDemoExamples.length]; break;
      case 'Home': next = homeDemoExamples[0]; break;
      case 'End': next = homeDemoExamples[homeDemoExamples.length - 1]; break;
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
    if (attempt !== copyAttempt) return;
    if (result === 'copied') consent.trackBehavioral('output_copied', {surface:'home_demo'});
    setFeedback(result);
  }

  return (
    <section class="home-conversion-demo" aria-labelledby={titleId} lang={props.locale}>
      <div class="home-conversion-demo-header">
        <span class="home-conversion-demo-mark"><Braces size={18} aria-hidden="true" /></span>
        <h2 id={titleId}>{labels().title}</h2>
        <span class="home-conversion-demo-preview">{labels().preview}</span>
      </div>
      <div class="home-conversion-demo-tabs" role="tablist" aria-labelledby={titleId}>
        <For each={homeDemoExamples}>{(key) => {
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
      <For each={homeDemoExamples}>{(key) => {
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
                <MarkdownViewToggle locale={props.locale} view={view()} onViewChange={setView} />
                <button type="button" class="home-conversion-demo-copy" onClick={() => void copyMarkdown(example().markdown)}>
                  <Show when={feedback() === 'copied'} fallback={<Copy size={14} aria-hidden="true" />}>
                    <Check size={14} aria-hidden="true" />
                  </Show>
                  {labels().copy}
                </button>
              </div>
              <div class="home-conversion-demo-pane">
                <Show when={active() === key && view() === 'preview'} fallback={
                  <pre tabIndex={0} aria-label={props.locale === 'en' && key === 'pdf' ? 'Example Markdown' : labels().output}><code><For each={example().markdown.split('\n')}>{(line, index) => (
                    <span classList={{
                      'home-conversion-demo-anchor': line.startsWith('<!--'),
                      'home-conversion-demo-heading': line.startsWith('## '),
                    }}>{index() > 0 ? '\n' : ''}{line}</span>
                  )}</For></code></pre>
                }>
                  <MarkdownPreview markdown={example().markdown} locale={props.locale} />
                </Show>
              </div>
            </div>
            <CampaignLink class="home-conversion-demo-link inline-link" href={localizedPath('/studio', props.locale) + '?sample=' + key}>
              {example().trySample} <ArrowRight size={14} aria-hidden="true" />
            </CampaignLink>
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
