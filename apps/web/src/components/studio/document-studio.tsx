import { ArrowRight, Braces, Check, Download, FileText, LockKeyhole, Upload } from 'lucide-solid';
import { A } from '@solidjs/router';
import CitePreview from './cite-preview';
import { createSignal, For, onCleanup, Show } from 'solid-js';
import MarkdownPreview from './markdown-preview';
import type { ConversionSource } from '@/lib/analytics';
import type { Locale } from '@/lib/marketing-locale';
import { studioCopy, studioStats } from '@/lib/studio-copy';
import { findQuoteMatches, normalizeQuote, type QuoteMatch } from '@/lib/studio-quote';
import { CopyButton } from '../copy';
import { useCookieConsent } from '../cookie-consent';

type WorkerResult = {markdown:string; format:string; tokens:number; unit_noun:string; units:number; source_units:{label:string; text:string}[]; ms:number};
type Result = WorkerResult & {sourceKind: ConversionSource; sourceBytes: number; markdownBytes: number};
const accepted = '.pdf,.docx,.pptx,.xlsx,.html,.htm,.epub,.odt,.ods,.odp,.csv,.tsv,.txt,.md,.rtf,.eml';
const maxBytes = 32 * 1024 * 1024;
const SAMPLE = {brief:{name:'brief.html', url:'/samples/brief.html'}, pdf:{name:'sample.pdf', url:'/samples/sample.pdf'}};

function downloadMarkdown(filename: string, markdown: string) {
  const stem = (filename.replace(/\.[^.]+$/, '') || 'document').replace(/[\/:\\]/g, '_');
  const url = URL.createObjectURL(new Blob([markdown], { type: 'text/markdown;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = stem + '.md';
  document.body.append(link);
  link.click();
  link.remove();
  // The browser reads this URL when it starts the download, after the click.
  setTimeout(() => URL.revokeObjectURL(url), 30_000);
}

export default function DocumentStudio(props: { locale: Locale }) {
  const t = () => studioCopy[props.locale];
  const consent = useCookieConsent();
  const [result, setResult] = createSignal<Result | null>(null);
  const noSelectableText = () => {
    const output = result();
    return output?.format === 'pdf' && !output.source_units.some(unit => unit.text.trim());
  };
  const [file, setFile] = createSignal<string | null>(null);
  const [pending, setPending] = createSignal(false);
  const [notice, setNotice] = createSignal('');
  const [quote, setQuote] = createSignal('');
  const [match, setMatch] = createSignal<QuoteMatch[] | null>(null);
  const [view, setView] = createSignal<'markdown' | 'preview'>('markdown');
  const [drag, setDrag] = createSignal(false);
  const [stamp, setStamp] = createSignal<string | null>(null);
  const [videoUrl, setVideoUrl] = createSignal<string | null>(null);
  let video: HTMLVideoElement | undefined;
  let worker: Worker | undefined;
  let controller: AbortController | undefined;
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let input: HTMLInputElement | undefined;
  let localVideoUrl: string | undefined;

  function stop() {
    clearTimeout(timer); timer = undefined;
    controller?.abort(); controller = undefined;
    worker?.terminate(); worker = undefined;
    generation++;
  }
  function releaseVideo() {
    if (localVideoUrl) URL.revokeObjectURL(localVideoUrl);
    localVideoUrl = undefined;
  }
  onCleanup(() => { stop(); releaseVideo(); });

  async function convert(name: string, source: File | string) {
    stop(); const run = generation;
    releaseVideo();
    setResult(null); setView('markdown'); setQuote(''); setMatch(null); setFile(name); setNotice(''); setStamp(null); setVideoUrl(null);
    if (source instanceof File && source.type.startsWith('video/')) {
      localVideoUrl = URL.createObjectURL(source);
      setVideoUrl(localVideoUrl); setPending(false); return;
    }
    if (source instanceof File && (source.size === 0 || source.size > maxBytes)) {
      setPending(false); setNotice(t().sizeError); return;
    }
    const sourceKind = typeof source === 'string' ? 'sample' : 'local_file';
    setPending(true);
    const fail = (message: string) => { if (generation !== run) return; stop(); setPending(false); setNotice(message); };
    timer = setTimeout(() => fail(t().timeoutError), 35_000);
    try {
      controller = new AbortController();
      const content = typeof source === 'string' ? await fetch(source, {signal:controller.signal}).then((r) => { if (!r.ok) throw new Error(); return r.arrayBuffer(); }) : await source.arrayBuffer();
      if (generation !== run) return;
      const sourceBytes = content.byteLength;
      const w = new Worker('/studio-worker.js', {type:'module'}); worker = w;
      w.onmessage = ({data}) => {
        if (run !== generation || data.id !== run) return;
        if (data.outcome !== 'ok') { fail(t().readError); return; }
        const output: WorkerResult = data.result;
        const markdownBytes = new TextEncoder().encode(output.markdown).length;
        setResult({...output, sourceKind, sourceBytes, markdownBytes}); setPending(false); stop();
        consent.trackBehavioral('document_converted', {
          file_format:output.format, source_kind:sourceKind, unit_count:output.units, token_count:output.tokens, duration_ms:output.ms,
        });
      };
      w.onerror = () => fail(t().startError);
      w.postMessage({id:run, name, bytes:content}, [content]);
    } catch { fail(t().openError); }
  }

  function useFiles(files: FileList | null) {
    if (!files?.length) return;
    void convert(files[0].name, files[0]);
    if (input) input.value = '';
  }
  function sample(key:'brief'|'pdf') { const choice = SAMPLE[key]; void convert(choice.name,choice.url); }

  function searchQuote(event: SubmitEvent) {
    event.preventDefault();
    const output = result();
    const needle = normalizeQuote(quote());
    if (!output || needle.length < 3) return;
    const matches = findQuoteMatches(output.source_units, needle);
    setMatch(matches);
    consent.trackBehavioral('quote_searched', {matched:matches.length > 0});
  }

  return <div class="studio">
    <div class="studio-bar"><span class="studio-title"><Braces size={17} /> {t().bar}</span><span class="studio-private"><LockKeyhole size={13} /> {t().private}</span></div>
    <input ref={(element) => { input = element; }} type="file" hidden accept={`${accepted},video/*`} onChange={(e)=>useFiles(e.currentTarget.files)} />
    <div class="studio-controls">
      <button class="button button-primary" type="button" onClick={()=>input?.click()}><Upload size={15} /> {t().open}</button>
      <button class="button" type="button" onClick={()=>sample('brief')}>{t().sampleDoc}</button>
      <button class="button" type="button" onClick={()=>sample('pdf')}>{t().samplePdf}</button><button class="button" type="button" onClick={()=>{stop();releaseVideo();setFile('demo.mp4');setVideoUrl('/demo.mp4');setResult(null);setMatch(null);setNotice('');setPending(false);setStamp(null);}}>{t().sampleVideo}</button>
    </div>
    <div class={'studio-workspace' + (drag() ? ' studio-drag' : '')} onDragOver={(e)=>{e.preventDefault();setDrag(true);}} onDragLeave={()=>setDrag(false)} onDrop={(e)=>{e.preventDefault();setDrag(false);useFiles(e.dataTransfer?.files ?? null);}}>
      <div class="studio-output-header">
        <div class="studio-label"><span><FileText size={15} aria-hidden="true" /> {file() ?? t().inputOutput}</span><Show when={result()}>{(output) => <span>{output().format.toUpperCase()} · {output().ms} ms</span>}</Show></div>
        <Show when={noSelectableText() ? null : result()}>{output => <div class="studio-output-actions">
          <div class="studio-view" role="group" aria-label={t().view}>
            <button type="button" aria-pressed={view() === 'markdown'} onClick={() => setView('markdown')}>Markdown</button>
            <button type="button" aria-pressed={view() === 'preview'} onClick={() => setView('preview')}>{t().preview}</button>
          </div>
          <span class="studio-stats">{studioStats(props.locale, output().units, output().unit_noun, output().tokens, output().sourceBytes, output().markdownBytes)}</span>
          <div class="studio-export">
            <CopyButton text={output().markdown} label={t().copyMarkdown} copied={t().copied} copyFailed={t().copyFailed} analytics={{surface:'studio_output'}} />
            <button type="button" class="button copy-button" onClick={() => {
              const {markdown, format, sourceKind} = output();
              downloadMarkdown(file() ?? 'document', markdown);
              consent.trackBehavioral('output_downloaded', {file_format:format, source_kind:sourceKind});
            }}><Download size={15} aria-hidden="true" />{t().download}</button>
          </div>
        </div>}</Show>
      </div>
      {pending() ? <div class="studio-empty" role="status">{t().converting} <button type="button" class="inline-link" onClick={()=>{stop();setPending(false);setFile(null);}}>{t().cancel}</button></div> : notice() ? <p class="studio-empty" role="alert">{notice()}</p> : noSelectableText() ? <div class="studio-empty" role="status"><strong>{t().scanned}</strong><span class="muted">{t().ocr}<A href="/docs/cli" class="inline-link">{t().ocrLink}</A>{t().ocrEnd}</span></div> : result() ? <div class="studio-output-pane">
        <Show when={view() === 'preview'} fallback={<pre class="studio-code" tabIndex={0} aria-label={t().markdown}>{result()?.markdown}</pre>}>
          <MarkdownPreview markdown={result()?.markdown ?? ''} locale={props.locale} />
        </Show>
      </div> : videoUrl() ? <div class="studio-media"><video ref={(element) => { video = element; }} controls playsinline preload="metadata" src={videoUrl() ?? undefined} aria-label={t().video}/><button type="button" class="button" onClick={()=>{ if (video?.readyState) setStamp(`[${file()} @ ${video.currentTime.toFixed(3)}s]`); }}>{t().mark}</button>{stamp() && <div class="flex flex-wrap gap-2"><code>{stamp()}</code><CopyButton text={stamp() ?? ''} label={t().copyTimestamp} copied={t().copied} copyFailed={t().copyFailed} analytics={{surface:'studio_timestamp'}} /></div>}</div> : <div class="studio-empty"><Upload size={24} /><strong>{t().drop}</strong><span class="muted">{t().formats}</span></div>}
    </div>
    <div class="studio-bottom"><span>{t().localMarkdown}</span><span>RUST / WASM</span></div>
    {result() && !noSelectableText() && <form class="studio-quote stack" onSubmit={searchQuote}>
      <div><h2>{t().quoteTitle}</h2><p class="muted">{t().quoteHelp}</p></div>
      <label class="field-label">{t().quoteLabel}<input class="field" required minLength={3} maxLength={500} placeholder={t().quotePlaceholder} value={quote()} onInput={(e)=>{setQuote(e.currentTarget.value);setMatch(null);}} /></label>
      <button class="button justify-self-start" type="submit">{t().find} <ArrowRight size={15} /></button>
      <Show when={match()}>{matches => <div class="studio-quote-results">
        <p class="notice" role="status">{matches().length ? <><Check class="inline" size={16} aria-hidden="true" /> {t().found(matches().map(item => item.label))}</> : t().noMatch}</p>
        <Show when={matches().length}>
          <ul class="studio-excerpts">
            <For each={matches()}>{item => <li>
              <span class="studio-excerpt-label">{t().sourceLabel(item.label)}</span>
              <p>{item.before}<mark>{item.matched}</mark>{item.after}</p>
            </li>}</For>
          </ul>
        </Show>
      </div>}</Show>
    </form>}
    <CitePreview locale={props.locale} />
  </div>;
}
