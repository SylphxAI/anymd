import { PageMeta } from '@/components/page-meta';
import { ArrowRight, ArrowUpRight, AudioLines, FileText, Globe, ShieldCheck, Terminal } from 'lucide-solid';
import { A } from '@solidjs/router';
import { CopyButton } from '@/components/copy';
import { SessionAction } from '@/components/session';
import DocumentStudio from '@/components/studio/document-studio';

const install = 'npx -y @sylphx/anymd setup';

export default function Home() {
  return <>
    <PageMeta canonical="/" />
    <section class="hero wrap">
      <div class="hero-copy">
        <div class="eyebrow"><span class="status-dot" /> OPEN SOURCE · RUST · MCP</div>
        <h1>Documents in.<br/><em>Markdown out.</em></h1>
        <p class="hero-summary">Turn PDFs, Word files, web pages and audio into clean Markdown your agents can use. One local engine, for the terminal and every MCP client.</p>
        <div class="hero-actions"><SessionAction primary /><A href="/docs" class="button">Install MCP / CLI <ArrowUpRight size={16} /></A></div>
        <div class="install-line"><code><span>$ </span>{install}</code><CopyButton text={install} label="Copy install command" /></div>
      </div>
      <div class="hero-preview"><DocumentStudio compact /><A href="/studio" class="preview-link">Open Studio <ArrowRight size={15} /></A></div>
    </section>
    <section class="format-strip" aria-label="Supported inputs"><div class="wrap formats">
      <span class="eyebrow">READ FROM</span><span><FileText /> PDF</span><span><FileText /> DOCX</span><span><Globe /> URLs</span><span><AudioLines /> Audio</span><span><FileText /> EPUB</span><span class="format-rule"><ShieldCheck /> Local by default</span>
    </div></section>
    <section class="wrap home-workflow"><div><span class="eyebrow">01 / THE WORKFLOW</span><h2>From source file<br/>to citable text.</h2></div><div class="workflow-steps">
      <div><span>01</span><div><h3>Read what’s there.</h3><p>Extract useful text and structure from reports, spreadsheets and presentations. Keep page boundaries for PDFs.</p></div></div>
      <div><span>02</span><div><h3>Use it in your own tools.</h3><p>Pipe Markdown into your shell or let Claude, Cursor and other MCP clients call Anymd directly.</p></div></div>
      <div><span>03</span><div><h3>Check it against the source.</h3><p>With Pro, verify a PDF quote or cite an exact video frame using the native CLI or MCP server.</p></div></div>
    </div></section>
    <section class="wrap home-bottom"><div class="pro-panel"><div><span class="eyebrow"><Terminal size={16} /> ANYMD PRO</span><h2>Give your citations a source.</h2><p>PDF quote verification and video frame evidence. Works offline. Pay once; no subscription.</p></div><A href="/pricing" class="button button-primary">See Pro pricing <ArrowRight size={16} /></A></div></section>
  </>;
}
