import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findQuoteMatches, normalizeQuote } from './studio-quote';

const find = (texts: string[], query: string) => findQuoteMatches(texts.map((text, i) => ({ label: 'page ' + (i + 1), text })), normalizeQuote(query));

test('matches the first normalized phrase per source unit, in order, up to three units', () => {
  assert.deepEqual(find(['nothing', 'Start Ｆｉｅｌｄ\n\tNotes; field notes again', 'FIELD NOTES', 'Field Notes.', 'Field notes elsewhere'], '  field   notes '), [
    { label: 'page 2', before: 'Start ', matched: 'Field Notes', after: '; field notes again' },
    { label: 'page 3', before: '', matched: 'FIELD NOTES', after: '' },
    { label: 'page 4', before: '', matched: 'Field Notes', after: '.' },
  ]);
  assert.deepEqual(find(['Quarterly', 'Report'], 'Quarterly Report'), []);
  assert.deepEqual(find(['Field Notes'], 'Fi'), []);
});

test('keeps highlight offsets across Unicode case expansion', () => {
  assert.deepEqual(find(['İ before İSTANBUL, end'], 'İstanbul'), [
    { label: 'page 1', before: 'İ before ', matched: 'İSTANBUL', after: ', end' },
  ]);
});

test('centers a short excerpt on the match without splitting emoji or long matches', () => {
  const [excerpt] = find(['🌊'.repeat(90) + 'Field Notes' + '🌱'.repeat(90)], 'Field Notes');
  assert.equal(excerpt.before, '…' + '🌊'.repeat(32));
  assert.equal(excerpt.after, '🌱'.repeat(33) + '…');
  const long = 'x'.repeat(160);
  assert.equal(find(['Start ' + long + ' end'], long)[0].matched, long);
});
