import assert from 'node:assert/strict';
import { test } from 'node:test';
import { studioCopy, studioStats } from './studio-copy';

test('formats Studio counts by locale and preserves English plurals', () => {
  assert.equal(studioStats('zh-TW', 2, 'page', 1240), '2 頁 · 約 1,240 個 token');
  assert.equal(studioStats('zh-TW', 1, 'sheet', 12), '1 張工作表 · 約 12 個 token');
  assert.equal(studioStats('en', 1, 'page', 12), '1 page · ~12 tokens');
  assert.equal(studioStats('en', 2, 'page', 1240), '2 pages · ~1,240 tokens');
});

test('translates source references without changing source titles or sheet names', () => {
  assert.equal(studioCopy['zh-TW'].found(['document', 'page 2', 'slide 1', 'sheet 報表', 'chapter 3: Revenue']),
    '找到引文：文件、第 2 頁、第 1 張投影片、工作表「報表」、第 3 章：Revenue。');
  assert.equal(studioCopy.en.found(['page 1', 'page 3']), 'Found in page 1, page 3.');
});
