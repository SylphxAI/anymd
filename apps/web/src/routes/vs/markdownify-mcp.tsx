import { ArrowDown, ArrowRight } from 'lucide-solid';
import { For } from 'solid-js';
import { useCookieConsent } from '@/components/cookie-consent';
import { HomeConversionDemo } from '@/components/home-conversion-demo';
import { MarkdownifySetup } from '@/components/markdownify-setup';
import { PageMeta } from '@/components/page-meta';
import { Faq, StructuredData, faqSchema, freeOffer, softwareSchema } from '@/components/search-details';
import { CampaignLink } from '@/lib/attribution-link';

const surface = 'seo_vs-markdownify-mcp';
const path = '/vs/markdownify-mcp';
const title = 'Markdownify MCP alternative: Anymd for local conversion';
const description = 'Compare Anymd and Markdownify MCP for PDF, Office, web and media conversion. Try local Rust and WebAssembly conversion, or set up the CLI and MCP server.';

const comparison = [
  ['Install','One Rust executable for the CLI and MCP server. Install with npm or download a release binary.','The upstream setup uses Bun to install and build the TypeScript server. It also installs Python MarkItDown in a virtual environment.'],
  ['Documents','PDF, Word, PowerPoint, Excel, EPUB, HTML, OpenDocument and more.','PDF, Word, PowerPoint and Excel tools, plus a tool to retrieve existing Markdown.'],
  ['Images and media','Local OCR for images and scans. Audio and video can yield metadata, subtitles or local transcripts, depending on the input and installed tools.','Image and audio tools with the full local install. The slim Docker image omits those dependencies.'],
  ['Web content','Convert HTML files or fetch URLs. There are no dedicated Bing or YouTube transcript tools.','Dedicated tools for web pages, Bing results and YouTube transcripts.'],
  ['Markdown output','Repeatable formatting for unchanged documents with the same version and options. PDFs include page anchors for citations.','The document tools use MarkItDown for conversion. Web tools use their own extraction paths.'],
] as const;

const questions = [
  {
    question: 'What is the difference between Anymd and Markdownify MCP?',
    answer: 'Anymd is a Rust CLI and MCP server in one executable. Markdownify MCP is a TypeScript MCP server that uses Python MarkItDown for document conversion and offers dedicated Bing and YouTube tools.',
  },
  {
    question: 'Does Anymd upload my files?',
    answer: 'No. Studio converts supported documents in your browser with WebAssembly, and the local CLI and MCP server read files on your machine by default. Fetching a URL, downloading a model or opting into a remote provider uses the network.',
  },
  {
    question: 'Which formats can I convert with Anymd?',
    answer: 'The CLI and MCP server read PDF, DOCX, PPTX, Excel, EPUB, HTML, images and other formats. Image and scanned PDF OCR and media transcription may need local tools or models. Studio supports a smaller set of document formats.',
  },
  {
    question: 'Do I need an API key to use Anymd with an MCP client?',
    answer: 'No. Run anymd mcp from your MCP client over stdio. The npm launcher needs Node.js; the standalone executable does not.',
  },
] as const;

export default function MarkdownifyComparison() {
  const consent = useCookieConsent();
  return <article class="wrap comparison-page">
    <PageMeta title={title} description={description} canonical={path} />
    <StructuredData nodes={[softwareSchema(freeOffer()), faqSchema(path, questions)]} />
    <header class="comparison-hero">
      <div>
        <CampaignLink class="eyebrow inline-link" href="/vs">Converter comparisons</CampaignLink>
        <h1>Anymd vs <em>Markdownify MCP</em></h1>
        <p>Choose Anymd for fast local file conversion from one Rust CLI and MCP server. Choose Markdownify MCP if your agent needs dedicated Bing search or YouTube transcript tools.</p>
        <div class="hero-actions">
          <CampaignLink href="/studio?sample=pdf" class="button button-primary">Open the PDF sample <ArrowRight size={16} /></CampaignLink>
          <a href="#install" class="button">Get the CLI and MCP server <ArrowDown size={16} /></a>
        </div>
      </div>
      <HomeConversionDemo locale="en" copySurface="markdownify_mcp_demo" />
    </header>

    <section class="comparison-section" aria-labelledby="comparison-title">
      <h2 id="comparison-title">How the two tools compare</h2>
      <table class="comparison-table" aria-labelledby="comparison-title">
        <thead><tr><th scope="col">What to check</th><th scope="col">Anymd</th><th scope="col">Markdownify MCP</th></tr></thead>
        <tbody><For each={comparison}>{([label, anymd, markdownify]) => <tr><th scope="row">{label}</th><td>{anymd}</td><td>{markdownify}</td></tr>}</For></tbody>
      </table>
      <p class="muted">This comparison covers <a class="inline-link" href="https://github.com/zcaceres/markdownify-mcp">zcaceres/markdownify-mcp</a>. <CampaignLink class="inline-link" href="/docs/benchmarks">AgentDocBench</CampaignLink> measures Anymd and the standalone MarkItDown converter, not the Markdownify MCP server. There is no direct speed result for the two MCP servers.</p>
    </section>

    <section class="comparison-section" id="install" aria-labelledby="install-title">
      <h2 id="install-title">Install Anymd locally</h2>
      <p>The CLI and MCP server are free and open source. npm installs the launcher and native binary for your platform. If you do not use Node.js, download the <a class="inline-link" href="https://github.com/SylphxAI/anymd/releases/latest">standalone binary</a> instead.</p>
      <MarkdownifySetup />
      <p>See <CampaignLink class="inline-link" href="/docs">setup by client</CampaignLink>, <CampaignLink class="inline-link" href="/docs/tools">MCP tools</CampaignLink> and the <CampaignLink class="inline-link" href="/docs/formats">full format list</CampaignLink>.</p>
    </section>

    <section class="comparison-section" aria-labelledby="pro-title">
      <h2 id="pro-title">Verify what your agent cites</h2>
      <p>Anymd Pro checks PDF quotes against the cited page and captures exact video frames. Pay US$29 once, with no subscription. Sign in, pay securely, then run <code>anymd login</code> with the same account to activate Pro.</p>
      <CampaignLink class="button button-primary" href={'/pricing?entry_surface=' + surface} onClick={() => consent.trackBehavioral('pro_cta_clicked', {entry_surface:surface})}>See Anymd Pro <ArrowRight size={16} /></CampaignLink>
    </section>

    <section class="comparison-section" aria-labelledby="faq-title">
      <h2 id="faq-title">Frequently asked questions</h2>
      <Faq questions={questions} />
    </section>
  </article>;
}
