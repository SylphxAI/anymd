#!/usr/bin/env bun

import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';

const [
  input,
  page = '',
  regionId = '',
  evidenceId = '',
  languages = '',
  mode = 'success',
  marker = '',
] = process.argv.slice(2);
if (!input) process.exit(2);
readFileSync(input);
if (mode === 'fail' || regionId === 'fail') process.exit(7);
if (mode === 'sleep') await Bun.sleep(60_000);
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
if (mode === 'oversize') {
  if (marker) appendFileSync(marker, `${regionId}\n`);
  if (regionId !== 'rich-first') {
    await new Promise<void>((resolve) => {
      process.stdout.write('B'.repeat(1024 * 1024 + 1), () => resolve());
    });
    process.exit(0);
  }
}

if (regionId === 'plain') {
  process.stdout.write(` ${'A'.repeat(1002)} `);
} else if (regionId === 'formula') {
  process.stdout.write(
    JSON.stringify({
      kind: 'FORMULA',
      description: ` page ${page} ${evidenceId} `,
      formula: { latex: ' x^2 + y^2 ', ascii_math: ' x^2+y^2 ', confidence: 95 },
      warnings: [' check notation ', '', 3],
    })
  );
} else {
  process.stdout.write(
    JSON.stringify({
      kind: ' SPREADSHEET ',
      description: ` region ${regionId} languages ${languages} `,
      text: ' extracted table ',
      markdown: ' |A|B| ',
      confidence: 87,
      warnings: [' provider note ', '', 3],
      table: {
        rows: [
          ['A', 2, true, null],
          ['C', { ignored: true }],
        ],
        markdown: ' |A|2| ',
        csv: ' A,2 ',
        cells: [
          {
            text: ' value ',
            row: 1,
            column: 2,
            rowspan: 2,
            colspan: 3,
            confidence: 50,
            bbox: { left: 1, bottom: 2, right: 3, top: 4 },
          },
          { text: 'invalid', row: -1, column: 0 },
        ],
        confidence: 92,
      },
      formula: { ascii_math: ' x^2 ', confidence: 150 },
      chart: {
        title: ' Sales ',
        summary: ' Trend ',
        data_points: [{ x: 1, y: 2, nested: { ignored: true } }, null],
        x_axis: { label: ' Quarter ', unit: ' q ', min: 0, max: 4 },
        y_axis: { label: ' Revenue ', unit: ' GBP ' },
        series: [{ name: ' Main ', points: [{ x: 1, y: 2 }], confidence: 90 }],
        confidence: 88,
      },
    })
  );
}
