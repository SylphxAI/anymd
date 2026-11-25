import { CampaignLink } from '@/lib/attribution-link';
import { ArrowRight, ScanLine, ShieldCheck } from 'lucide-solid';
import { useCookieConsent } from '../cookie-consent';
import { PdfQuoteCrop } from './pdf-quote-crop';
import proof from './sample-cite-proof.json';

export default function CitePreview() {
  const consent = useCookieConsent();
  const surface = {entry_surface:'studio_cite_preview'} as const;
  return (
    <section class="cite-preview" aria-labelledby="studio-cite-heading">
      <div class="cite-preview-intro">
        <div class="stack">
          <span class="eyebrow"><ScanLine size={16} aria-hidden="true" /> PRO PREVIEW</span>
          <h2 id="studio-cite-heading">Verify a quote on the exact PDF page crop</h2>
        </div>
        <CampaignLink class="button button-primary" href="/pricing?entry_surface=studio_cite_preview" onClick={() => consent.trackBehavioral('pro_cta_clicked', surface)}>Get Anymd Pro — $29 once <ArrowRight size={16} aria-hidden="true" /></CampaignLink>
      </div>
      <p class="muted">Free quote search finds the page. With Pro, <code>cite_check</code> pins the exact wording to a box you can check.</p>
      <details class="cite-preview-details" onToggle={event => {
        if (event.currentTarget.open) consent.trackBehavioral('paywall_view', surface);
      }}>
        <summary>See the verified quote in sample.pdf</summary>
        <div class="cite-preview-evidence">
          <figure class="cite-preview-figure">
            <figcaption><span>sample.pdf · page {proof.page}</span><span>PAGE CROP</span></figcaption>
            <PdfQuoteCrop label={`Page ${proof.page} crop highlighting “${proof.quote}”`} />
          </figure>
          <div class="cite-preview-match">
            <span class="cite-preview-badge"><ShieldCheck size={16} aria-hidden="true" /> Exact quote verified</span>
            <blockquote>“{proof.quote}”</blockquote>
            <p class="muted">Recorded from <code>cite_check</code> on the bundled sample. This verifies the wording and location, not the claim itself.</p>
          </div>
          <details class="cite-preview-json">
            <summary>View bounding box and PDF SHA-256</summary>
            <pre aria-label="Citation evidence JSON">{JSON.stringify(proof, null, 2)}</pre>
          </details>
        </div>
      </details>
      <p class="cite-preview-footnote muted">The native CLI and MCP server also verify your own PDFs and extract video frames with SHA-256 hashes.</p>
    </section>
  );
}
