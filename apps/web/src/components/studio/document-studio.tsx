import { ArrowRight, Braces, Check, FileText, LockKeyhole, Upload } from 'lucide-solid';
import { A } from '@solidjs/router';
import { createSignal, onCleanup, Show } from 'solid-js';
import { CopyButton } from '../copy';

type Result = {markdown:string; format:string; tokens:number; unit_noun:string; units:number; source_units:{label:string; text:string}[]; ms:number};
const accepted = '.pdf,.docx,.pptx,.xlsx,.html,.htm,.epub,.odt,.ods,.odp,.csv,.tsv,.txt,.md,.rtf,.eml';
const maxBytes = 32 * 1024 * 1024;
const SAMPLE = {brief:{name:'brief.html', url:'/samples/brief.html'}, pdf:{name:'sample.pdf', url:'/samples/sample.pdf'}};
const normalize = (s: string) => s.normalize('NFKC').replace(/\s+/g, ' ').trim().toLowerCase();

function formatStats(result: Result) {
  return result.units + ' ' + result.unit_noun + (result.units === 1 ? '' : 's') + ' · ~' + result.tokens.toLocaleString('en-US') + ' tokens';
}

export default function DocumentStudio(props: { compact?: boolean }) {
  const [result, setResult] = createSignal<Result | null>(null);
  const [file, setFile] = createSignal<string | null>(null);
  const [pending, setPending] = createSignal(false);
  const [notice, setNotice] = createSignal('');
  const [quote, setQuote] = createSignal('');
  const [match, setMatch] = createSignal<string[] | null>(null);
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
    setResult(null); setQuote(''); setMatch(null); setFile(name); setNotice(''); setStamp(null); setVideoUrl(null);
    if (source instanceof File && source.type.startsWith('video/')) {
      localVideoUrl = URL.createObjectURL(source);
      setVideoUrl(localVideoUrl); setPending(false); return;
    }
    if (source instanceof File && (source.size === 0 || source.size > maxBytes)) {
      setPending(false); setNotice('Choose a non-empty document up to 32 MB. Use the native CLI for larger files.'); return;
    }
    setPending(true);
    const fail = (message: string) => { if (generation !== run) return; stop(); setPending(false); setNotice(message); };
    timer = setTimeout(() => fail('Conversion timed out. Try a smaller document or use the native CLI.'), 35_000);
    try {
      controller = new AbortController();
      const content = typeof source === 'string' ? await fetch(source, {signal:controller.signal}).then((r) => { if (!r.ok) throw new Error(); return r.arrayBuffer(); }) : await source.arrayBuffer();
      if (generation !== run) return;
      const w = new Worker('/studio-worker.js', {type:'module'}); worker = w;
      w.onmessage = ({data}) => {
        if (run !== generation || data.id !== run) return;
        if (data.outcome !== 'ok') { fail('Couldn’t read this document. Check its format, or try the same file with the native CLI.'); return; }
        setResult(data.result); setPending(false); stop();
      };
      w.onerror = () => fail('The browser converter could not start. Try again or use the native CLI.');
      w.postMessage({id:run, name, bytes:content}, [content]);
    } catch { fail('Couldn’t open this file. Try again or choose another.'); }
  }

  function useFiles(files: FileList | null) {
    if (!files?.length) return;
    void convert(files[0].name, files[0]);
    if (input) input.value = '';
  }
  function sample(key:'brief'|'pdf') { const choice = SAMPLE[key]; void convert(choice.name,choice.url); }

  // The homepage starts with an illustrative preview; every actual selection
  // replaces it with bytes parsed by the same engine used by the full Studio.
  return <div class={'studio ' + (props.compact ? 'studio-compact' : '')}>
    <div class="studio-bar"><span class="studio-title"><Braces size={17} /> {props.compact ? 'TRY THE CONVERTER' : 'DOCUMENT STUDIO'}</span><span class="studio-private"><LockKeyhole size={13} /> Runs in your browser</span></div>
    <input ref={(element) => { input = element; }} type="file" hidden accept={`${accepted},video/*`} onChange={(e)=>useFiles(e.currentTarget.files)} />
    <div class="studio-controls">
      <button class="button studio-upload" type="button" onClick={()=>input?.click()}><Upload size={15} /> Open a file</button>
      <button class="button" type="button" onClick={()=>sample('brief')}>Sample document</button>
      {!props.compact && <><button class="button" type="button" onClick={()=>sample('pdf')}>Sample PDF</button><button class="button" type="button" onClick={()=>{stop();releaseVideo();setFile('demo.mp4');setVideoUrl('/demo.mp4');setResult(null);setMatch(null);setNotice('');setPending(false);setStamp(null);}}>Sample video</button></>}
    </div>
    <div class={'studio-workspace' + (drag() ? ' studio-drag' : '')} onDragOver={(e)=>{e.preventDefault();setDrag(true);}} onDragLeave={()=>setDrag(false)} onDrop={(e)=>{e.preventDefault();setDrag(false);useFiles(e.dataTransfer?.files ?? null);}}>
      <div class="studio-label"><span><FileText size={15} /> {file() ?? (props.compact ? 'brief.html → Markdown' : 'INPUT / OUTPUT')}</span><Show when={result()}>{(output) => <span>{output().format.toUpperCase()} · {output().ms} ms</span>}</Show></div>
      {pending() ? <div class="studio-empty" role="status">Converting locally… <button type="button" class="inline-link" onClick={()=>{stop();setPending(false);setFile(null);}}>Cancel</button></div> : notice() ? <p class="studio-empty" role="alert">{notice()}</p> : result() ? <pre class="studio-code" tabIndex={0} aria-label="Markdown output">{result()?.markdown}</pre> : videoUrl() ? <div class="studio-media"><video ref={(element) => { video = element; }} controls playsinline preload="metadata" src={videoUrl() ?? undefined} aria-label="Local source video"/><button type="button" class="button" onClick={()=>{ if (video?.readyState) setStamp(`[${file()} @ ${video.currentTime.toFixed(3)}s]`); }}>Mark current timestamp</button>{stamp() && <div class="flex flex-wrap gap-2"><code>{stamp()}</code><CopyButton text={stamp() ?? ''} label="Copy timestamp" /></div>}</div> : props.compact ? <pre class="studio-code studio-demo" aria-label="Example Markdown"><span># Field Notes: Water Survey</span>{'\n\nThree teams sampled the lower river\nafter the spring rain.\n\n'}<span>## Findings</span>{'\n\n- Site B recorded the lowest turbidity.\n- Teams returned to Site A two weeks later.'}</pre> : <div class="studio-empty"><Upload size={24} /><strong>Drop a document here</strong><span class="muted">PDF, Office, HTML, EPUB, text and more. Up to 32 MB. No uploads.</span></div>}
    </div>
    <div class="studio-bottom">
      <Show when={result()} fallback={<><span>LOCAL FILE → MARKDOWN</span><span>RUST / WASM</span></>}>
        {(output) => <><span>{formatStats(output())}</span><CopyButton text={output().markdown} label="Copy Markdown" /></>}
      </Show>
    </div>
    {!props.compact && result() && <form class="studio-quote stack" onSubmit={(e)=>{e.preventDefault();const output = result(); const needle = normalize(quote()); if (output && needle.length >= 3) setMatch(output.source_units.filter((unit)=>normalize(unit.text).includes(needle)).map((unit)=>unit.label));}}>
      <div><h2>Find a quote in the source</h2><p class="muted">Matches extracted text within a single page, slide or section. Ignores case and spacing; it does not verify a paraphrase or run OCR.</p></div>
      <label class="field-label">Quote from the document<input class="field" required minLength={3} maxLength={500} placeholder="Paste a phrase to find…" value={quote()} onInput={(e)=>{setQuote(e.currentTarget.value);setMatch(null);}} /></label>
      <button class="button justify-self-start" type="submit">Find quote <ArrowRight size={15} /></button>
      {match() && <p class="notice" role="status">{match()?.length ? <><Check class="inline" size={16} /> Found in {match()?.join(', ')}.</> : 'No exact match within a single source unit.'}</p>}
    </form>}
    {!props.compact && <div class="studio-bottom studio-upgrade"><span>Need PDF evidence or hashed video frames? Use the native CLI or MCP server.</span><A class="inline-link" href="/pricing">See Anymd Pro <ArrowRight size={14} /></A></div>}
  </div>;
}
