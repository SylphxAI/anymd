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

export function PdfQuoteCrop(props: { label: string }) {
  return <svg viewBox={`0 0 ${cropWidth} ${cropHeight}`} role="img" aria-label={props.label}>
      <image href={crop} width={cropWidth} height={cropHeight} />
      <rect x={left - cropLeft} y={pageHeight - top - cropTop} width={right - left} height={top - bottom} rx="1" />
    </svg>;
}
