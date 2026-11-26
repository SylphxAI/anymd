import { ScanLine } from 'lucide-solid';
import { CampaignLink } from '@/lib/attribution-link';
import { translatedPaths } from '@/lib/marketing-locale';
import { PdfQuoteCrop } from './studio/pdf-quote-crop';
import frame from './pro-demo-frame.png';

// Decoded from public/demo.mp4 with the render_frame trim/scale/showinfo
// filter (stream 0, time base 1/12800). Requesting 4.001 s yields PTS 51328.
const frameSha256 = '06e5b9de39fc6fbe945bce346f75c413092628b99ae1ba254e8a6098d9ba58a1';

export function ProProof(props: { zh: boolean }) {
  return <section class="pro-proof" aria-labelledby="pro-proof-title">
    <h3 id="pro-proof-title"><ScanLine size={18} aria-hidden="true" />{props.zh ? '查看原始證據' : 'See the source evidence'}</h3>
    <div class="proof-grid">
      <figure class="proof-tile">
        <figcaption><span class="eyebrow">01 / PDF</span><strong>{props.zh ? '引文所在區域' : 'Quote bounding box'}</strong></figcaption>
        <div class="proof-image proof-pdf"><PdfQuoteCrop label={props.zh ? 'PDF 第 1 頁。「Quarterly Field Report」的引文以方框標示。' : 'Page 1 of the sample PDF with “Quarterly Field Report” boxed'} /></div>
        <p><a href="/samples/sample.pdf#page=1" class="inline-link">sample.pdf</a> · {props.zh ? '第 1 頁' : 'page 1'}</p>
        <p class="proof-detail">{props.zh ? '逐字核對並標出引文在原頁的位置。' : 'Exact wording, marked on the original page.'}</p>
        <CampaignLink href={(props.zh ? translatedPaths['/studio'] : '/studio') + '?sample=pdf#studio-cite-heading'} class="inline-link">{props.zh ? '查看引文座標' : 'Inspect citation proof'}</CampaignLink>
      </figure>
      <figure class="proof-tile">
        <figcaption><span class="eyebrow">02 / VIDEO</span><strong>{props.zh ? '帶有時間戳的影格' : 'Decoded video frame'}</strong></figcaption>
        <div class="proof-image proof-video"><img src={frame} width="1078" height="644" loading="lazy" alt={props.zh ? '示範影片在 4.010 秒時解碼的實際影格' : 'Actual frame decoded from the Anymd demo at 4.010 seconds'} /></div>
        <p><a href="/demo.mp4#t=4.010" class="inline-link">demo.mp4</a> · <time datetime="PT4.010S">4.010 s</time></p>
        <p class="proof-detail">{props.zh ? '請求 4.001 秒，取得其後第一個可解碼影格。' : 'First decodable frame at or after 4.001 s.'}</p>
        <details class="proof-hash"><summary>PNG SHA-256</summary><code>{frameSha256}</code></details>
      </figure>
    </div>
  </section>;
}
