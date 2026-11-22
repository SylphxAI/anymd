import { A } from '@solidjs/router';
import { ArrowRight, ScanLine, ShieldCheck } from 'lucide-solid';
import crop from './sample-cite-crop.png';
import proof from './sample-cite-proof.json';

// The raster shows page 1 of sample.pdf, clipped to x: 15–410, y: 27–118
// in top-left page coordinates. cite_check uses bottom-left PDF coordinates.
const pageHeight = 792;
const cropLeft = 15;
const cropTop = 27;
const cropWidth = 395;
const cropHeight = 91;
const { left, right, top, bottom } = proof.bbox;

export default function CitePreview() {
  return (
    <section class="cite-preview" aria-labelledby="studio-cite-heading">
      <div class="cite-preview-intro">
        <div class="stack">
          <span class="eyebrow"><ScanLine size={16} aria-hidden="true" /> PRO PREVIEW</span>
          <h2 id="studio-cite-heading">Verify a quote on the exact PDF page crop</h2>
        </div>
        <A class="button button-primary" href="/pricing?entry_surface=studio_cite_preview">Get Anymd Pro — $29 once <ArrowRight size={16} aria-hidden="true" /></A>
      </div>
      <p class="muted">Free quote search finds the page. With Pro, <code>cite_check</code> pins the exact wording to a box you can check.</p>
      <details class="cite-preview-details">
        <summary>See the verified quote in sample.pdf</summary>
        <div class="cite-preview-evidence">
          <figure class="cite-preview-figure">
            <figcaption><span>sample.pdf · page {proof.page}</span><span>PAGE CROP</span></figcaption>
            <svg viewBox={`0 0 ${cropWidth} ${cropHeight}`} role="img" aria-label={`Page ${proof.page} crop highlighting “${proof.quote}”`}>
              <image href={crop} width={cropWidth} height={cropHeight} />
              <rect x={left - cropLeft} y={pageHeight - top - cropTop} width={right - left} height={top - bottom} rx="1" />
            </svg>
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
