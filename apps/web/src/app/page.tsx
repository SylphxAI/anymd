import { ArrowRight, ArrowUpRight, AudioLines, FileText, Globe, ShieldCheck, Terminal } from 'lucide-react';
import Link from 'next/link';
import { CopyButton } from '@/components/copy';
import { SessionAction } from '@/components/session';
import DocumentStudio from '@/components/studio/document-studio';

export const metadata = { alternates: { canonical: '/' } };
const install = 'npx -y @sylphx/anymd setup';

export default function Home() {
  return <>
    <section className="hero wrap">
      <div className="hero-copy">
        <div className="eyebrow"><span className="status-dot" /> OPEN SOURCE · RUST · MCP</div>
        <h1>Documents in.<br/><em>Markdown out.</em></h1>
        <p className="hero-summary">Turn PDFs, Word files, web pages and audio into clean Markdown your agents can use. One local engine, for the terminal and every MCP client.</p>
        <div className="hero-actions"><SessionAction primary /><Link href="/docs" className="button">Install MCP / CLI <ArrowUpRight size={16} /></Link></div>
        <div className="install-line"><code><span>$ </span>{install}</code><CopyButton text={install} label="Copy install command" /></div>
      </div>
      <div className="hero-preview"><DocumentStudio compact /><Link href="/studio" className="preview-link">Open Studio <ArrowRight size={15} /></Link></div>
    </section>
    <section className="format-strip" aria-label="Supported inputs"><div className="wrap formats">
      <span className="eyebrow">READ FROM</span><span><FileText /> PDF</span><span><FileText /> DOCX</span><span><Globe /> URLs</span><span><AudioLines /> Audio</span><span><FileText /> EPUB</span><span className="format-rule"><ShieldCheck /> Local by default</span>
    </div></section>
    <section className="wrap home-workflow"><div><span className="eyebrow">01 / THE WORKFLOW</span><h2>From source file<br/>to citable text.</h2></div><div className="workflow-steps">
      <div><span>01</span><div><h3>Read what’s there.</h3><p>Extract useful text and structure from reports, spreadsheets and presentations. Keep page boundaries for PDFs.</p></div></div>
      <div><span>02</span><div><h3>Use it in your own tools.</h3><p>Pipe Markdown into your shell or let Claude, Cursor and other MCP clients call Anymd directly.</p></div></div>
      <div><span>03</span><div><h3>Check it against the source.</h3><p>With Pro, verify a PDF quote or cite an exact video frame using the native CLI or MCP server.</p></div></div>
    </div></section>
    <section className="wrap home-bottom"><div className="pro-panel"><div><span className="eyebrow"><Terminal size={16} /> ANYMD PRO</span><h2>Give your citations a source.</h2><p>PDF quote verification and video frame evidence. Works offline. Pay once; no subscription.</p></div><Link href="/pricing" className="button button-primary">See Pro pricing <ArrowRight size={16} /></Link></div></section>
  </>;
}
