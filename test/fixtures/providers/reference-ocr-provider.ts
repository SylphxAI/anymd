#!/usr/bin/env bun

import { readFileSync, writeFileSync } from 'node:fs';
import { PNG } from 'pngjs';

const [input, page = '0', languages = '', mode = 'success', marker = ''] = process.argv.slice(2);
if (!input) throw new Error('usage: reference-ocr-provider <input.png> <page> <languages>');
if (mode === 'fail') process.exit(7);
if (mode === 'sleep') await Bun.sleep(5_000);
if (mode === 'escaped-descendant') {
  if (marker) writeFileSync(marker, input);
  const command =
    process.platform === 'win32'
      ? [process.execPath, '-e', 'await Bun.sleep(5_000)']
      : ['python3', '-c', 'import os,time; os.setsid(); time.sleep(5)'];
  const descendant = Bun.spawn({
    cmd: command,
    stdin: 'ignore',
    stdout: 'inherit',
    stderr: 'inherit',
  });
  descendant.unref();
  process.exit(0);
}
const png = PNG.sync.read(readFileSync(input));

const words =
  page === '3'
    ? [
        ['Metric', 80, 1400, 176, 1420],
        ['Value', 320, 1400, 404, 1420],
        ['Revenue', 80, 1360, 200, 1380],
        ['24%', 320, 1360, 368, 1380],
      ].map(([text, left, bottom, right, top]) => ({
        text,
        confidence: 93,
        bounding_box: { left, bottom, right, top },
      }))
    : [
        {
          text: 'Reference',
          confidence: 91,
          bounding_box: { left: 20, bottom: 10, right: 100, top: 30 },
        },
      ];

process.stdout.write(
  JSON.stringify({
    text:
      page === '3'
        ? 'Metric Value\nRevenue 24%'
        : `Reference OCR page ${page} at ${String(png.width)}x${String(png.height)}`,
    confidence: 87,
    words,
    ...(languages ? { language: languages.split(',')[0] } : {}),
  })
);
