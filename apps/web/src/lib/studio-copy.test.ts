import assert from 'node:assert/strict';
import { test } from 'node:test';
import { formatBytes, studioCopy, studioStats } from './studio-copy';

test('formats Studio counts and source-to-Markdown sizes in both locales', () => {
  assert.equal(studioStats('zh-TW', 2, 'page', 1240, 21_000, 1400), '2 頁 · 約 1,240 個 token · 21 KB → 1.4 KB');
  assert.equal(studioStats('zh-TW', 1, 'sheet', 12, 30, 85), '1 張工作表 · 約 12 個 token · 30 B → 85 B');
  assert.equal(studioStats('en', 1, 'page', 12, 88, 31), '1 page · ~12 tokens · 88 B → 31 B');
  assert.equal(studioStats('en', 2, 'page', 1240, 21_000, 1400), '2 pages · ~1,240 tokens · 21 KB → 1.4 KB');
});

test('uses decimal units and preserves exact sub-kilobyte counts', () => {
  assert.equal(formatBytes(0, 'en'), '0 B');
  assert.equal(formatBytes(999, 'zh-TW'), '999 B');
  assert.equal(formatBytes(1000, 'en'), '1 KB');
  assert.equal(formatBytes(1_450_000, 'en'), '1.5 MB');
});

test('translates source references and common metadata without changing source titles or sheet names', () => {
  assert.equal(studioCopy['zh-TW'].found(['document', 'page 2', 'slide 1', 'sheet 報表', 'chapter 3: Revenue']),
    '找到引文：文件、第 2 頁、第 1 張投影片、工作表「報表」、第 3 章：Revenue。');
  assert.equal(studioCopy.en.found(['page 1', 'page 3']), 'Found in page 1, page 3.');
  assert.equal(studioCopy['zh-TW'].metadataLabel('pages'), '頁數');
  assert.equal(studioCopy['zh-TW'].metadataLabel('custom'), 'custom');
});
