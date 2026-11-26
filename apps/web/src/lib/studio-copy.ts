import type { Locale } from './marketing-locale';

export const studioCopy = {
  en: {
    title: 'Document Studio', heading: 'Document Studio.', eyebrow: 'WORKBENCH / LOCAL',
    description: 'Convert a PDF or document to Markdown in your browser. Files stay on your device.',
    intro: 'Convert documents to Markdown, find a quote in extracted text, or mark a video timestamp. Files stay on this device.',
    bar: 'DOCUMENT STUDIO', private: 'Runs in your browser',
    open: 'Open a file', sampleDoc: 'Sample document', samplePdf: 'Sample PDF', sampleVideo: 'Sample video',
    inputOutput: 'INPUT / OUTPUT', localMarkdown: 'LOCAL FILE → MARKDOWN',
    loading: 'Loading Studio…',
    drop: 'Drop a document here', formats: 'PDF, Office, HTML, EPUB, text and more. Up to 32 MB. No uploads.',
    converting: 'Converting locally…', cancel: 'Cancel',
    sizeError: 'Choose a non-empty document up to 32 MB. Use the native CLI for larger files.',
    timeoutError: 'Conversion timed out. Try a smaller document or use the native CLI.',
    readError: 'Couldn’t read this document. Check its format, or try the same file with the native CLI.',
    startError: 'The browser converter could not start. Try again or use the native CLI.',
    openError: 'Couldn’t open this file. Try again or choose another.',
    emptyPdf: 'No selectable text found in this PDF.',
    partialPdf: (missing: string[], pages: number) =>
      'Partial conversion: No selectable text found on ' + (missing.length <= 3
        ? new Intl.ListFormat('en').format(missing)
        : new Intl.NumberFormat('en').format(missing.length) + ' of ' + new Intl.NumberFormat('en').format(pages) + ' pages') + '.',
    ocr: 'For text in images, see the ', ocrLink: 'local OCR instructions', ocrEnd: '.',
    view: 'Output view', preview: 'Preview', previewOutput: 'Markdown preview', previewTable: 'Preview table',
    sourceLabel: (label: string) => label, metadataLabel: (key: string) => key,
    markdown: 'Markdown output', copyMarkdown: 'Copy Markdown', download: 'Download .md',
    copied: 'Copied', copyFailed: 'Select and copy the text',
    video: 'Local source video', mark: 'Mark current timestamp', copyTimestamp: 'Copy timestamp',
    quoteTitle: 'Find a quote in the source',
    quoteHelp: 'Matches extracted text within a single page, slide or section. Ignores case and spacing; it does not verify a paraphrase or run OCR.',
    quoteLabel: 'Quote from the document', quotePlaceholder: 'Paste a phrase to find…', find: 'Find quote',
    found: (labels: string[]) => 'Found in ' + labels.join(', ') + '.',
    noMatch: 'No exact match within a single source unit.',
    quoteSamples: 'Try a quote from this sample',
    quoteProofStart: 'Studio matches text on this device. Anymd Pro’s ',
    quoteProofEnd: ' shows the exact PDF page, bounding-box coordinates, and a crop you can check.',
    quoteSeeProof: 'See the sample proof', quoteSeePro: 'View Anymd Pro pricing',
    proof: {
      eyebrow: 'PRO PREVIEW', heading: 'Verify a quote on the exact PDF page crop',
      upgrade: (price: string) => 'Get Anymd Pro — ' + price + ' once',
      helpStart: 'Free quote search finds the page. With Pro, ', helpEnd: ' pins the exact wording to a box you can check.',
      see: 'See the verified quote in sample.pdf', page: (page: number) => 'page ' + page, crop: 'PAGE CROP',
      image: (page: number, quote: string) => 'Page ' + page + ' crop highlighting “' + quote + '”',
      verified: 'Exact quote verified', recordedStart: 'Recorded from ',
      recordedEnd: ' on the bundled sample. This verifies the wording and location, not the claim itself.',
      details: 'View bounding box and PDF SHA-256', json: 'Citation evidence JSON',
      footnote: 'The native CLI and MCP server also verify your own PDFs and extract video frames with SHA-256 hashes.',
    },
  },
  'zh-TW': {
    title: '轉檔工作室', heading: '轉檔工作室', eyebrow: '工作台／本機處理',
    description: '在瀏覽器中將 PDF 或其他文件轉成 Markdown，檔案不會離開你的裝置。',
    intro: '將文件轉成 Markdown、在擷取的文字中查找引文，或標記影片的時間戳記。檔案不會離開這台裝置。',
    bar: '轉檔工作室', private: '在瀏覽器中執行',
    open: '開啟檔案', sampleDoc: '範例文件', samplePdf: '範例 PDF', sampleVideo: '範例影片',
    inputOutput: '輸入／輸出', localMarkdown: '本機檔案 → Markdown',
    loading: '正在載入轉檔工作室…',
    drop: '將文件拖曳到這裡', formats: '支援 PDF、Office、HTML、EPUB、純文字等格式。上限 32 MB，不會上傳檔案。',
    converting: '正在本機轉檔…', cancel: '取消',
    sizeError: '請選擇非空白且不超過 32 MB 的文件。較大的檔案請使用本機 CLI 處理。',
    timeoutError: '轉檔逾時。請試用較小的文件，或改用本機 CLI。',
    readError: '無法讀取這份文件。請確認檔案格式，或用本機 CLI 開啟同一個檔案。',
    startError: '瀏覽器無法啟動轉檔工具。請再試一次，或改用本機 CLI。',
    openError: '無法開啟這個檔案。請再試一次，或選擇其他檔案。',
    emptyPdf: '這份 PDF 沒有可選取的文字。',
    partialPdf: (missing: string[], pages: number) =>
      '部分轉檔結果：' + (missing.length <= 3
        ? missing.map(zhSourceLabel).join('、')
        : new Intl.NumberFormat('zh-TW').format(pages) + ' 頁中有 ' + new Intl.NumberFormat('zh-TW').format(missing.length) + ' 頁') + '未找到可選取的文字。',
    ocr: '若圖片中有文字，請參閱', ocrLink: '本機 OCR 操作說明', ocrEnd: '。',
    view: '輸出檢視', preview: '預覽', previewOutput: 'Markdown 預覽', previewTable: '預覽表格',
    sourceLabel: zhSourceLabel, metadataLabel: (key: string) => zhMetadata.get(key) ?? key,
    markdown: 'Markdown 輸出', copyMarkdown: '複製 Markdown', download: '下載 .md',
    copied: '已複製', copyFailed: '請選取文字後自行複製',
    video: '本機來源影片', mark: '標記目前的時間戳記', copyTimestamp: '複製時間戳記',
    quoteTitle: '在原文中查找引文',
    quoteHelp: '比對擷取的文字，範圍限於同一頁、同一張投影片或同一節。忽略大小寫與空格，不比對改寫內容，也不執行 OCR。',
    quoteLabel: '文件中的引文', quotePlaceholder: '貼上要查找的片語…', find: '查找引文',
    found: (labels: string[]) => '找到引文：' + labels.map(zhSourceLabel).join('、') + '。',
    noMatch: '未在同一頁、投影片或章節內找到完全相符的文字。',
    quoteSamples: '試查範例中的引文',
    quoteProofStart: '工作室在這台裝置上比對文字。Anymd Pro 的 ',
    quoteProofEnd: ' 會標示引文的 PDF 頁碼和方框座標，並提供局部畫面供你核對。',
    quoteSeeProof: '查看範例引文證據', quoteSeePro: '查看 Anymd Pro 價格',
    proof: {
      eyebrow: 'PRO 功能預覽', heading: '在 PDF 原頁的局部畫面核對引文',
      upgrade: (price: string) => '購買 Anymd Pro · 一次付費 ' + price,
      helpStart: '免費查找可定位引文所在的頁面。Pro 的 ', helpEnd: ' 會框出原文位置，方便你逐字核對。',
      see: '查看 sample.pdf 中已核對的引文', page: (page: number) => '第 ' + page + ' 頁', crop: '頁面局部',
      image: (page: number, quote: string) => '第 ' + page + ' 頁局部畫面，方框標示「' + quote + '」',
      verified: '引文已逐字核對', recordedStart: '以下是在內附範例上執行 ',
      recordedEnd: ' 的結果，只核對引文的字句與位置，不查證引文本身的說法。',
      details: '查看引文座標與 PDF 的 SHA-256', json: '引文證據 JSON',
      footnote: '本機 CLI 和 MCP 伺服器也能核對你自己的 PDF 引文，並擷取影片影格及其 SHA-256 雜湊值。',
    },
  },
} satisfies Record<Locale, unknown>;

function zhSourceLabel(label: string) {
  if (label === 'document') return '文件';
  const [, kind, number, title] = /^(page|slide|chapter) (\d+)(?:: (.+))?$/.exec(label) ?? [];
  if (kind === 'page') return '第 ' + number + ' 頁' + (title ? '：' + title : '');
  if (kind === 'slide') return '第 ' + number + ' 張投影片' + (title ? '：' + title : '');
  if (kind === 'chapter') return '第 ' + number + ' 章' + (title ? '：' + title : '');
  return label.startsWith('sheet ') ? '工作表「' + label.slice(6) + '」' : label;
}

const zhUnit = new Map([['page', '頁'], ['slide', '張投影片'], ['sheet', '張工作表'], ['chapter', '章'], ['section', '節']]);

const zhMetadata = new Map([
  ['source', '來源'], ['format', '格式'], ['title', '標題'], ['pages', '頁數'], ['slides', '投影片數'],
  ['sheets', '工作表數'], ['chapters', '章數'], ['sections', '節數'], ['author', '作者'],
]);

export function formatBytes(bytes: number, locale: Locale) {
  const [value, unit] = bytes >= 1_000_000 ? [bytes / 1_000_000, 'MB'] : bytes >= 1_000 ? [bytes / 1_000, 'KB'] : [bytes, 'B'];
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value) + ' ' + unit;
}

export function studioStats(locale: Locale, units: number, noun: string, tokens: number, sourceBytes: number, markdownBytes: number) {
  const number = new Intl.NumberFormat(locale);
  const counts = locale === 'zh-TW'
    ? number.format(units) + ' ' + (zhUnit.get(noun) ?? '節') + ' · 約 ' + number.format(tokens) + ' 個 token'
    : number.format(units) + ' ' + noun + (units === 1 ? '' : 's') + ' · ~' + number.format(tokens) + ' tokens';
  return counts + ' · ' + formatBytes(sourceBytes, locale) + ' → ' + formatBytes(markdownBytes, locale);
}
