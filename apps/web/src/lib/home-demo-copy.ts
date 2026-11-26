import type { Locale } from './marketing-locale';

export const homeDemoExamples = ['pdf', 'document', 'video'] as const;
export type HomeDemoExample = (typeof homeDemoExamples)[number];

type DemoCopy = {
  title: string;
  preview: string;
  output: string;
  copy: string;
  copied: string;
  copyFailed: string;
  notice: string;
  nativePro: string;
  examples: Record<HomeDemoExample, { tab: string; file: string; description: string; markdown: string; trySample: string }>;
};

// Authored examples, not live output. Page and slide anchors mirror native read;
// the frame citation summarizes the timestamps used in docs/tasks/extract-video-frame.md.
export const homeDemoCopy: Record<Locale, DemoCopy> = {
  en: {
    title: 'Conversion examples',
    preview: 'Illustrative preview',
    output: 'Markdown output',
    copy: 'Copy Markdown',
    copied: 'Markdown copied.',
    copyFailed: 'Couldn’t copy. Select the example text to copy it manually.',
    notice: 'This preview does not convert files.',
    nativePro: 'Pro · Native CLI / MCP',
    examples: {
      pdf: {
        tab: 'PDF',
        file: 'water-survey.pdf',
        trySample: 'Try a PDF in Studio',
        description: 'Page markers and a table, kept in reading order.',
        markdown: `<!-- page 3 -->

## River survey

|Site|Turbidity (NTU)|
|-|-|
|North bank|4.2|
|South bank|2.8|`,
      },
      document: {
        tab: 'Multi-page document',
        file: 'field-brief.pptx',
        trySample: 'Try a document in Studio',
        description: 'Slide headings and lists stay in source order.',
        markdown: `<!-- slide 1 -->

## Field brief
- Sample both banks after the rain.

<!-- slide 2 -->

## Next visit
- Recheck the south bank in two weeks.`,
      },
      video: {
        tab: 'Video frame',
        file: 'demo.mkv',
        trySample: 'Try a video in Studio',
        description: 'A Markdown citation based on exact frame metadata.',
        markdown: `## Frame citation

- Source: demo.mkv
- Requested: 00:01.031
- Decoded: 00:01.100
- Image: 320 × 240 PNG
- PNG SHA-256: omitted in this preview`,
      },
    },
  },
  'zh-TW': {
    title: '轉換範例',
    preview: '示意預覽',
    output: 'Markdown 輸出',
    copy: '複製 Markdown',
    copied: '已複製 Markdown。',
    copyFailed: '無法複製，請選取範例文字後手動複製。',
    notice: '此預覽不會轉換檔案。',
    nativePro: 'Pro · 本機 CLI／MCP',
    examples: {
      pdf: {
        tab: 'PDF',
        file: '水質調查.pdf',
        trySample: '在工作室試用 PDF 範例',
        description: '依閱讀順序整理內容，保留頁碼標記與表格。',
        markdown: `<!-- page 3 -->

## 河川水質調查

|採樣地點|濁度（NTU）|
|-|-|
|北岸|4.2|
|南岸|2.8|`,
      },
      document: {
        tab: '多頁文件',
        file: '採樣簡報.pptx',
        trySample: '在工作室試用文件範例',
        description: '依原始投影片的順序保留標題和清單。',
        markdown: `<!-- slide 1 -->

## 採樣計畫
- 雨後到兩岸採集水樣。

<!-- slide 2 -->

## 下次勘查
- 兩週後回到南岸複測。`,
      },
      video: {
        tab: '影片影格',
        file: 'demo.mkv',
        trySample: '在工作室試用影片範例',
        description: '用精確影格的資訊撰寫 Markdown 引用。',
        markdown: `## 影格引用

- 來源：demo.mkv
- 指定時間：00:01.031
- 實際解碼時間：00:01.100
- 影像：320 × 240 PNG
- PNG 的 SHA-256：此預覽省略`,
      },
    },
  },
};
