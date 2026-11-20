'use client';

import { ArrowRight, Braces, Check, FileText, LockKeyhole, Upload } from 'lucide-react';
import Link from 'next/link';
import { useEffect, useRef, useState } from 'react';
import { CopyButton } from '../copy';

type Result = {markdown:string; format:string; tokens:number; unit_noun:string; units:number; source_units:{label:string; text:string}[]; ms:number};
const accepted = '.pdf,.docx,.pptx,.xlsx,.html,.htm,.epub,.odt,.ods,.odp,.csv,.tsv,.txt,.md,.rtf,.eml';
const maxBytes = 32 * 1024 * 1024;
const SAMPLE = {brief:{name:'brief.html', url:'/samples/brief.html'}, pdf:{name:'sample.pdf', url:'/samples/sample.pdf'}};
const normalize = (s: string) => s.normalize('NFKC').replace(/\s+/g, ' ').trim().toLowerCase();

export default function DocumentStudio({compact = false}: {compact?:boolean}) {
  const [result, setResult] = useState<Result | null>(null);
  const [file, setFile] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState('');
  const [quote, setQuote] = useState('');
  const [match, setMatch] = useState<string[] | null>(null);
  const [drag, setDrag] = useState(false);
  const [stamp, setStamp] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const worker = useRef<Worker | null>(null);
  const controller = useRef<AbortController | null>(null);
  const generation = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const input = useRef<HTMLInputElement>(null);

  function stop() {
    if (timer.current) clearTimeout(timer.current);
    controller.current?.abort();
    worker.current?.terminate(); worker.current = null;
    generation.current++;
  }
  useEffect(() => stop, []);
  useEffect(() => () => { if (videoUrl) URL.revokeObjectURL(videoUrl); }, [videoUrl]);

  async function convert(name: string, source: File | string) {
    stop(); const run = generation.current;
    setResult(null); setQuote(''); setMatch(null); setFile(name); setNotice(''); setStamp(null); setVideoUrl(null);
    if (source instanceof File && source.type.startsWith('video/')) {
      setVideoUrl(URL.createObjectURL(source)); setPending(false); return;
    }
    if (source instanceof File && (source.size === 0 || source.size > maxBytes)) {
      setPending(false); setNotice('Choose a non-empty document up to 32 MB. Use the native CLI for larger files.'); return;
    }
    setPending(true);
    const fail = (message: string) => { if (generation.current !== run) return; stop(); setPending(false); setNotice(message); };
    timer.current = setTimeout(() => fail('Conversion timed out. Try a smaller document or use the native CLI.'), 35_000);
    try {
      controller.current = new AbortController();
      const content = typeof source === 'string' ? await fetch(source, {signal:controller.current.signal}).then((r) => { if (!r.ok) throw new Error(); return r.arrayBuffer(); }) : await source.arrayBuffer();
      if (generation.current !== run) return;
      const w = new Worker('/studio-worker.js', {type:'module'}); worker.current = w;
      w.onmessage = ({data}) => {
        if (run !== generation.current || data.id !== run) return;
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
    if (input.current) input.current.value = '';
  }
  function sample(key:'brief'|'pdf') { const choice = SAMPLE[key]; void convert(choice.name,choice.url); }

  // The homepage starts with an illustrative preview; every actual selection
  // replaces it with bytes parsed by the same engine used by the full Studio.
  return <div className={'studio ' + (compact ? 'studio-compact' : '')}>
    <div className="studio-bar"><span className="studio-title"><Braces size={17} /> {compact ? 'TRY THE CONVERTER' : 'DOCUMENT STUDIO'}</span><span className="studio-private"><LockKeyhole size={13} /> Runs in your browser</span></div>
    <input ref={input} type="file" hidden accept={`${accepted},video/*`} onChange={(e)=>useFiles(e.target.files)} />
    <div className="studio-controls">
      <button className="button studio-upload" type="button" onClick={()=>input.current?.click()}><Upload size={15} /> Open a file</button>
      <button className="button" type="button" onClick={()=>sample('brief')}>Sample document</button>
      {!compact && <><button className="button" type="button" onClick={()=>sample('pdf')}>Sample PDF</button><button className="button" type="button" onClick={()=>{stop();setFile('demo.mp4');setVideoUrl('/demo.mp4');setResult(null);setMatch(null);setNotice('');setPending(false);setStamp(null);}}>Sample video</button></>}
    </div>
    <div className={'studio-workspace' + (drag ? ' studio-drag' : '')} onDragOver={(e)=>{e.preventDefault();setDrag(true);}} onDragLeave={()=>setDrag(false)} onDrop={(e)=>{e.preventDefault();setDrag(false);useFiles(e.dataTransfer.files);}}>
      <div className="studio-label"><span><FileText size={15} /> {file ?? (compact ? 'brief.html → Markdown' : 'INPUT / OUTPUT')}</span>{result && <span>{result.format.toUpperCase()} · {result.ms} ms</span>}</div>
      {pending ? <div className="studio-empty" role="status">Converting locally… <button type="button" className="inline-link" onClick={()=>{stop();setPending(false);setFile(null);}}>Cancel</button></div> : notice ? <p className="studio-empty" role="alert">{notice}</p> : result ? <pre className="studio-code" tabIndex={0} aria-label="Markdown output">{result.markdown}</pre> : videoUrl ? <div className="studio-media"><video ref={video} controls playsInline preload="metadata" src={videoUrl} aria-label="Local source video"/><button type="button" className="button" onClick={()=>{ if (video.current?.readyState) setStamp(`[${file} @ ${video.current.currentTime.toFixed(3)}s]`); }}>Mark current timestamp</button>{stamp && <div className="flex flex-wrap gap-2"><code>{stamp}</code><CopyButton text={stamp} label="Copy timestamp" /></div>}</div> : compact ? <pre className="studio-code studio-demo" aria-label="Example Markdown"><span># Field Notes: Water Survey</span>{'\n\nThree teams sampled the lower river\nafter the spring rain.\n\n'}<span>## Findings</span>{'\n\n- Site B recorded the lowest turbidity.\n- Teams returned to Site A two weeks later.'}</pre> : <div className="studio-empty"><Upload size={24} /><strong>Drop a document here</strong><span className="muted">PDF, Office, HTML, EPUB, text and more. Up to 32 MB. No uploads.</span></div>}
    </div>
    <div className="studio-bottom"><span>{result ? `${result.units} ${result.unit_noun}${result.units === 1 ? '' : 's'} · ~${result.tokens.toLocaleString('en-US')} tokens` : 'LOCAL FILE → MARKDOWN'}</span>{result ? <CopyButton text={result.markdown} label="Copy Markdown" /> : <span>RUST / WASM</span>}</div>
    {!compact && result && <form className="studio-quote stack" onSubmit={(e)=>{e.preventDefault();const needle=normalize(quote);if (needle.length >= 3) setMatch(result.source_units.filter((unit)=>normalize(unit.text).includes(needle)).map((unit)=>unit.label));}}>
      <div><h2>Find a quote in the source</h2><p className="muted">Matches extracted text within a single page, slide or section. Ignores case and spacing; it does not verify a paraphrase or run OCR.</p></div>
      <label className="field-label">Quote from the document<input className="field" required minLength={3} maxLength={500} placeholder="Paste a phrase to find…" value={quote} onChange={(e)=>{setQuote(e.target.value);setMatch(null);}} /></label>
      <button className="button justify-self-start" type="submit">Find quote <ArrowRight size={15} /></button>
      {match && <p className="notice" role="status">{match.length ? <><Check className="inline" size={16} /> Found in {match.join(', ')}.</> : 'No exact match within a single source unit.'}</p>}
    </form>}
    {!compact && <div className="studio-bottom studio-upgrade"><span>Need PDF evidence or hashed video frames? Use the native CLI or MCP server.</span><Link className="inline-link" href="/pricing">See Anymd Pro <ArrowRight size={14} /></Link></div>}
  </div>;
}
