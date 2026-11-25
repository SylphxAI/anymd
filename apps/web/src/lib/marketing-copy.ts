import type { Question } from '@/components/search-details';
import type { Locale } from './marketing-locale';

export const homeCopy = {
  en: {
    eyebrow: 'OPEN SOURCE · RUST · MCP', title: 'Documents in.', accent: 'Markdown out.',
    summary: 'Turn PDFs, Word files, web pages and audio into clean Markdown your agents can use. One local engine for the terminal and MCP clients.',
    tryStudio: 'Try Studio Free', getPro: 'Get Anymd Pro', openStudio: 'Open Studio',
    install: 'Installation options', copyInstall: 'Copy install command', copyPip: 'Copy pip install command', copyUvx: 'Copy uvx command', or: 'or', copied: 'Copied', copyFailed: 'Select and copy the text',
    formats: 'Supported inputs', localFiles: 'STUDIO · LOCAL FILES', audio: 'Audio', urls: 'Web URLs: CLI / MCP',
    workflow: '01 / THE WORKFLOW', workflowTitle: 'From source file', workflowAccent: 'to citable text.',
    steps: [
      ['Read what’s there.', 'Extract useful text and structure from reports, spreadsheets and presentations. Keep page boundaries for PDFs.'],
      ['Use it in your own tools.', 'Pipe Markdown into your shell or let Claude, Cursor and other MCP clients call Anymd directly.'],
      ['Check it against the source.', 'With Pro, verify a PDF quote or cite an exact video frame using the native CLI or MCP server.'],
    ],
    proTitle: 'Give your citations a source.', proDetail: 'PDF quote verification and video frame evidence. Works offline. Pay once; no subscription.', seePrice: 'See Pro pricing',
    faq: 'Frequently asked questions', questions: [
      { question: 'Can I convert a file for free?', answer: 'Yes. Try supported files in Studio or install the free, open-source CLI and MCP server. You do not need an account for local conversion.' },
      { question: 'Does Studio upload my document?', answer: 'No. Studio uses WebAssembly to convert supported files in your browser. Use the CLI or MCP server for web URLs.' },
      { question: 'What does Anymd Pro add?', answer: 'Pro verifies PDF quotes against exact page locations and supplies evidence for video frames from the native CLI or MCP server.' },
    ],
  },
  'zh-TW': {
    eyebrow: '開放原始碼 · RUST · MCP', title: '讀取文件，', accent: '輸出 Markdown',
    summary: '將 PDF、Word 文件、網頁和音訊轉成易讀的 Markdown，讓 AI 工具直接使用。終端機和 MCP 用戶端共用同一套本機轉檔引擎。',
    tryStudio: '免費試用轉檔工作室', getPro: '選購 Anymd Pro', openStudio: '開啟轉檔工作室',
    install: '安裝方式', copyInstall: '複製安裝指令', copyPip: '複製 pip 安裝指令', copyUvx: '複製 uvx 指令', or: '或', copied: '已複製', copyFailed: '請選取指令後手動複製',
    formats: '支援的輸入格式', localFiles: 'STUDIO · 本機檔案', audio: '音訊', urls: '網頁網址：CLI／MCP',
    workflow: '01 / 使用流程', workflowTitle: '從原始文件', workflowAccent: '到可查證的引用',
    steps: [
      ['保留文件結構', '從報告、試算表和簡報擷取文字與結構。轉換 PDF 時也會保留分頁。'],
      ['接到你慣用的工具', '在終端機把 Markdown 傳給下一個指令，或讓 Claude、Cursor 等 MCP 用戶端直接呼叫 Anymd。'],
      ['回到原始資料查證', '使用 Pro，即可透過本機 CLI 或 MCP 伺服器核對 PDF 引文，或引用影片中的確切影格。'],
    ],
    proTitle: '讓每筆引用都能查證', proDetail: '核對 PDF 引文，或擷取影片影格作為佐證。可離線使用，買斷制，不必訂閱。', seePrice: '查看 Pro 價格',
    faq: '常見問題', questions: [
      { question: '可以免費轉檔嗎？', answer: '可以。你可在 Studio 轉換支援的檔案，或安裝免費且開放原始碼的 CLI 和 MCP 伺服器。在本機轉檔不需要帳號。' },
      { question: 'Studio 會上傳我的文件嗎？', answer: '不會。Studio 透過 WebAssembly 在瀏覽器中轉換支援的檔案。若要直接處理網頁網址，請使用 CLI 或 MCP 伺服器。' },
      { question: 'Anymd Pro 多了哪些功能？', answer: '在本機 CLI 或 MCP 伺服器使用 Pro，即可查證 PDF 引文出自哪一頁、哪個位置，或擷取影片影格作為佐證。' },
    ],
  },
} satisfies Record<Locale, object>;

export const pricingCopy = {
  en: {
    title: 'Pricing', description: 'Compare free local Markdown conversion with Anymd Pro: one payment for PDF quote verification and exact video-frame citations.', eyebrow: 'PLANS / 01', heading: 'Open source first.',
    intro: 'Convert documents for free. Add Pro when you need to show exactly where a quote or video frame came from.',
    cancelled: 'Checkout was closed. No changes were made to your plan.', recover: 'Link a legacy purchase',
    free: 'Free OSS', freeLabel: 'CORE TOOLKIT', freeDetails: 'MIT licensed. Run locally, with no account or API key.', install: 'Install MCP / CLI',
    pro: 'Anymd Pro', proLabel: 'SOURCE VERIFICATION', oneTime: 'Tax included. Pay once; no subscription.',
    freeFeatures: ['PDF, Office, EPUB, web and media conversion', 'Command-line tools and MCP server', 'Page boundaries and structured Markdown'],
    proFeatures: ['Everything in Free OSS', 'PDF cite-check with exact page & bounding-box proof', 'Hashed video-frame timeline extraction & citations', 'Works offline after activation on each machine'],
    faq: 'Frequently asked questions', costQuestion: 'How much does Anymd Pro cost?',
    costAnswer: (price: string) => `Anymd Pro costs ${price} once, including tax. There is no subscription. You can select another supported currency before checkout.`,
    questions: [
      { question: 'How do I activate Pro?', answer: 'Sign in and pay securely, then run anymd login. Use the same account in your browser to activate the CLI and MCP server on that machine.' },
      { question: 'What can I do for free?', answer: 'Convert supported documents in Studio or use the open-source CLI and MCP server for local conversion. No account or API key is needed.' },
      { question: 'What does Pro add to the CLI and MCP server?', answer: 'Pro can verify PDF quotes against the exact page and location, and find and cite exact video frames. These tools run locally.' },
      {
        question: 'Does Anymd upload my documents? Can I use Pro offline?',
        answer: 'Studio converts files in your browser without uploading them. The CLI and MCP server process documents locally unless you explicitly choose a remote provider. Once you activate Pro on a machine, you can use its local Pro features offline.',
      },
      {
        question: 'Does Pro include updates? Where can I get an invoice or request a refund?',
        answer: [
          'Your one-time licence works with future releases that include the Pro features. Sign in and open ',
          { text: 'Billing', href: '/billing' },
          ' to find your invoices. To request a refund, email ',
          { text: 'hi@sylphx.com', href: 'mailto:hi@sylphx.com' },
          '. See the ',
          { text: 'Pro terms', href: '/legal/pro-terms' },
          ' for cancellation and refund rules. Your statutory rights still apply.',
        ],
      },
    ],
  },
  'zh-TW': {
    title: '方案與定價', description: '比較 Anymd 免費轉檔與 Pro 買斷方案。升級後可查證 PDF 引文的確切位置，並引用影片影格。', eyebrow: '方案 / 01', heading: '免費轉檔，按需升級',
    intro: '轉換文件不用付費。需要指出引文的確切位置，或引用影片中的特定影格時，再升級至 Pro。',
    cancelled: '你已離開結帳畫面，方案並未變更。', recover: '連結先前的購買紀錄',
    free: '免費版', freeLabel: '開放原始碼', freeDetails: '採用 MIT 授權。在本機執行，不需要帳號或 API 金鑰。', install: '安裝 MCP／CLI',
    pro: 'Anymd Pro', proLabel: '來源查證', oneTime: '價格已含稅，一次付清，不必訂閱。',
    freeFeatures: ['轉換 PDF、Office 文件、EPUB、網頁和媒體檔案', '命令列工具與 MCP 伺服器', '保留分頁與 Markdown 結構'],
    proFeatures: ['包含免費版的所有功能', '核對 PDF 引文的頁碼與頁面上的精確位置', '擷取影片時間軸，引用影格並保留雜湊值', '在各台裝置啟用後皆可離線使用'],
    faq: '常見問題', costQuestion: 'Anymd Pro 要多少錢？',
    costAnswer: (price: string) => `Anymd Pro 的買斷價格為 ${price}，已含稅，不必訂閱。你可以在結帳前改用其他支援的幣別。`,
    questions: [
      { question: '如何啟用 Pro？', answer: '登入並完成付款後，請執行 anymd login，再以同一個帳號在瀏覽器登入，即可在該裝置啟用 CLI 和 MCP 伺服器的 Pro 功能。' },
      { question: '免費版可以做什麼？', answer: '你可以在 Studio 轉換支援的文件，也可以用開放原始碼的 CLI 和 MCP 伺服器在本機轉檔。不需要帳號或 API 金鑰。' },
      { question: 'CLI 和 MCP 伺服器的 Pro 版多了哪些功能？', answer: 'Pro 能核對 PDF 引文出自哪一頁、哪個位置，也能找出並引用影片中的確切影格。這些工具都在本機執行。' },
      {
        question: 'Anymd 會上傳我的文件嗎？Pro 可以離線使用嗎？',
        answer: 'Studio 在瀏覽器中轉檔，不會上傳檔案。CLI 和 MCP 伺服器會在本機處理文件，除非你主動選用遠端服務。每台裝置啟用 Pro 後，就能離線使用 Pro 的本機功能。',
      },
      {
        question: 'Pro 買斷後還能更新嗎？要去哪裡查看發票或申請退款？',
        answer: [
          'Pro 一次付費，授權也適用於日後仍提供 Pro 功能的版本。登入後可前往',
          { text: '帳務頁面', href: '/billing' },
          '查看發票。若要申請退款，請寄信至 ',
          { text: 'hi@sylphx.com', href: 'mailto:hi@sylphx.com' },
          '。取消購買及退款的適用條件請見 ',
          { text: 'Pro 條款', href: '/legal/pro-terms' },
          '。你的法定權利不受影響。',
        ],
      },
    ],
  },
} satisfies Record<Locale, { questions: readonly Question[] } & Record<string, unknown>>;
