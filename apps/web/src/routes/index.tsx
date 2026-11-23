import { PageMeta } from '@/components/page-meta';
import { ArrowRight, ArrowUpRight, AudioLines, FileText, Globe, ShieldCheck, Terminal } from 'lucide-solid';
import { A } from '@solidjs/router';
import { CopyButton } from '@/components/copy';
import { Faq, StructuredData, faqSchema, softwareSchema, freeOffer, proOffer, type Question } from '@/components/search-details';
import { usdPriceLabel } from '@/lib/catalog';
import DocumentStudio from '@/components/studio/document-studio';

const npmSetup = 'npx -y @sylphx/anymd setup';
const pipInstall = 'pip install anymd';
const uvxRun = 'uvx anymd';

const questions = [
  { question: 'Can I convert a file for free?', answer: 'Yes. Try supported files in Studio or install the free, open-source CLI and MCP server. You do not need an account for local conversion.' },
  { question: 'Does Studio upload my document?', answer: 'No. Studio uses WebAssembly to convert supported files in your browser.' },
  { question: 'What does Anymd Pro add?', answer: 'Pro verifies PDF quotes against exact page locations and supplies evidence for video frames from the native CLI or MCP server.' },
] satisfies Question[];

export default function Home() {
  return <>
    <PageMeta canonical="/" />
    <StructuredData nodes={[softwareSchema(freeOffer, proOffer), faqSchema('/', questions)]} />
    <section class="hero wrap">
      <div class="hero-copy">
        <div class="eyebrow"><span class="status-dot" /> OPEN SOURCE · RUST · MCP</div>
        <h1>Documents in.<br/><em>Markdown out.</em></h1>
        <p class="hero-summary">Turn PDFs, Word files, web pages and audio into clean Markdown your agents can use. One local engine, for the terminal and every MCP client.</p>
        <div class="hero-actions"><A href="/studio" class="button button-primary">Try Studio Free <ArrowRight size={16} /></A><A href="/pricing" class="button">Get Anymd Pro — {usdPriceLabel}</A></div>
        <div class="hero-install" role="group" aria-label="Installation options">
          <div class="install-line">
            <code><span aria-hidden="true">$ </span>{npmSetup}</code>
            <CopyButton text={npmSetup} label="Copy install command" compact />
          </div>
          <div class="install-line">
            <div class="install-command"><code>{pipInstall}</code><CopyButton text={pipInstall} label="Copy pip install command" compact /></div>
            <div class="install-command"><span>or</span><code>{uvxRun}</code><CopyButton text={uvxRun} label="Copy uvx command" compact /></div>
            <a class="install-pypi" href="https://pypi.org/project/anymd/">PyPI <ArrowUpRight size={12} aria-hidden="true" /></a>
          </div>
        </div>
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
    <section class="wrap home-faq" aria-labelledby="faq-title">
      <h2 id="faq-title">Frequently asked questions</h2>
      <Faq questions={questions} />
    </section>
  </>;
}
