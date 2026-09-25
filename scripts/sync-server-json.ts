#!/usr/bin/env bun
import { readFileSync, writeFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync('package.json', 'utf8')) as { version: string };
const matrix = JSON.parse(
  readFileSync('docs/specs/pure-rust-capability-matrix.json', 'utf8')
) as {
  productTruth?: { dropInFor3014?: boolean };
};
const server = JSON.parse(readFileSync('server.json', 'utf8')) as {
  version: string;
  packages: Array<{ version: string }>;
};

// MCP registry metadata tracks the npm package version.
server.version = pkg.version;
server.packages[0].version = pkg.version;
writeFileSync('server.json', `${JSON.stringify(server, null, 2)}\n`);

const rustServerLib = 'crates/anymd/src/lib.rs';
const rustSource = readFileSync(rustServerLib, 'utf8');
const dropIn = matrix.productTruth?.dropInFor3014 === true;

// Until sole-runtime cutover, pure-Rust initialize must keep an experimental
// identity that is not the published npm package line. Do not rewrite it on
// every package version bump; tests and clients rely on the experimental marker.
const experimentalVersion = '0.0.0-pure-rust-experimental';
const targetRustVersion = dropIn ? pkg.version : experimentalVersion;

const nextRust = rustSource.replace(
  /pub const SERVER_VERSION: &str = "[^"]+";/,
  `pub const SERVER_VERSION: &str = "${targetRustVersion}";`
);
if (nextRust === rustSource && !rustSource.includes(`"${targetRustVersion}"`)) {
  throw new Error(
    `Failed to sync SERVER_VERSION in ${rustServerLib} to ${targetRustVersion}`
  );
}
if (nextRust !== rustSource) {
  writeFileSync(rustServerLib, nextRust);
}

console.log(
  JSON.stringify(
    {
      profile: 'sync_server_json',
      packageVersion: pkg.version,
      serverJsonVersion: server.version,
      rustServerVersion: targetRustVersion,
      dropInFor3014: dropIn,
      pass: true,
    },
    null,
    2
  )
);
