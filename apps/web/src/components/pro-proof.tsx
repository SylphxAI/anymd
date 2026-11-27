import { ScanLine, ShieldCheck } from 'lucide-solid';
import { CampaignLink } from '@/lib/attribution-link';
import { translatedPaths } from '@/lib/marketing-locale';
import { PdfQuoteCrop } from './studio/pdf-quote-crop';
import proof from './studio/sample-cite-proof.json';
import frame from './pro-demo-frame.png';

// Decoded from public/demo.mp4 with the render_frame trim/scale/showinfo
// filter (stream 0, time base 1/12800). Requesting 4.001 s yields PTS 51328.
const frameSha256 = '06e5b9de39fc6fbe945bce346f75c413092628b99ae1ba254e8a6098d9ba58a1';

/** The recorded cite_check crop of the published sample.pdf, shown next to Buy. */
export function PdfQuoteEvidence(props: { zh: boolean }) {
  return <figure class="pdf-evidence">
    <div class="pdf-evidence-crop"><PdfQuoteCrop label={props.zh ? `sample.pdf 第 ${proof.page} 頁。「${proof.quote}」的引文以方框標示。` : `Page ${proof.page} of the sample PDF with “${proof.quote}” boxed`} /></div>
    <figcaption><span class="pdf-evidence-verdict"><ShieldCheck size={15} aria-hidden="true" />{props.zh ? '引文已逐字核對' : 'Exact quote verified'}</span><span>{props.zh ? '錄製自 ' : 'Recorded on '}<a href={`/samples/sample.pdf#page=${proof.page}`} class="inline-link">sample.pdf</a> · {props.zh ? `第 ${proof.page} 頁` : `page ${proof.page}`}</span></figcaption>
  </figure>;
}

/** The PDF hash and the Studio replay of the same recorded result. */
export function PdfQuoteEvidenceDetails(props: { zh: boolean }) {
  return <div class="pdf-evidence-more">
    <details class="proof-hash"><summary>PDF SHA-256</summary><code>{proof.sha256}</code></details>
    <CampaignLink href={(props.zh ? translatedPaths['/studio'] : '/studio') + '?sample=pdf#studio-cite-heading'} class="inline-link">{props.zh ? '查看引文座標' : 'Inspect citation proof'}</CampaignLink>
  </div>;
}

export function ProProof(props: { zh: boolean }) {
  return <section class="pro-proof" aria-labelledby="pro-proof-title">
    <h3 id="pro-proof-title"><ScanLine size={18} aria-hidden="true" />{props.zh ? '影片也能引用' : 'Video frames too'}</h3>
    <figure class="proof-tile proof-tile-row">
      <div class="proof-image proof-video"><img src={frame} width="1078" height="644" loading="lazy" alt={props.zh ? '示範影片在 4.010 秒時解碼的實際影格' : 'Actual frame decoded from the Anymd demo at 4.010 seconds'} /></div>
      <figcaption><span class="eyebrow">VIDEO</span><strong>{props.zh ? '帶有時間戳的影格' : 'Decoded video frame'}</strong></figcaption>
      <p><a href="/demo.mp4#t=4.010" class="inline-link">demo.mp4</a> · <time datetime="PT4.010S">4.010 s</time></p>
      <p class="proof-detail">{props.zh ? '請求 4.001 秒，取得其後第一個可解碼影格。' : 'First decodable frame at or after 4.001 s.'}</p>
      <details class="proof-hash"><summary>PNG SHA-256</summary><code>{frameSha256}</code></details>
    </figure>
  </section>;
}
