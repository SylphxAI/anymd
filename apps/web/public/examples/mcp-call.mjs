// A one-call stdio MCP client for the task guides. Node.js 18+, no client SDK.
// Run from a directory with @sylphx/anymd@8.5.5 installed locally.
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';

const [tool, rawArguments] = process.argv.slice(2);
if (!['read', 'outline', 'search', 'inspect'].includes(tool) || !rawArguments) {
  throw new Error('Usage: node mcp-call.mjs <tool> <JSON arguments>');
}
const args = JSON.parse(rawArguments);
const root = resolve('node_modules/@sylphx/anymd');
const installed = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
if (installed.version !== '8.5.5') throw new Error('Install @sylphx/anymd@8.5.5 first');

const server = spawn(
  process.execPath,
  [resolve(root, 'bin/anymd.js'), 'mcp', `--allow-dir=${process.cwd()}`],
  { stdio: ['pipe', 'pipe', 'inherit'] }
);
const lines = createInterface({ input: server.stdout });
const pending = new Map();
let nextId = 0;

lines.on('line', (line) => {
  const response = JSON.parse(line);
  const handler = pending.get(response.id);
  if (!handler) return;
  pending.delete(response.id);
  if (response.error) handler.reject(new Error(JSON.stringify(response.error)));
  else handler.resolve(response.result);
});
server.on('error', (error) => {
  for (const handler of pending.values()) handler.reject(error);
});
server.on('exit', (code) => {
  for (const handler of pending.values()) handler.reject(new Error(`Server exited: ${code}`));
});
const send = (message) => server.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
const request = (method, params) => new Promise((resolveResult, reject) => {
  const id = ++nextId;
  pending.set(id, { resolve: resolveResult, reject });
  send({ id, method, params });
});
const timer = setTimeout(() => {
  for (const handler of pending.values()) handler.reject(new Error('MCP request timed out'));
  server.kill();
}, 90000);

try {
  await request('initialize', {
    protocolVersion: '2024-11-05', capabilities: {},
    clientInfo: { name: 'anymd-task-example', version: '1.0.0' },
  });
  send({ method: 'notifications/initialized' });
  const result = await request('tools/call', { name: tool, arguments: args });
  // Save returned image blocks locally rather than filling the terminal with base64.
  let image = 0;
  for (const block of result.content ?? []) {
    if (block.type !== 'image' || block.mimeType !== 'image/png') continue;
    const file = `frame-${++image}.png`;
    writeFileSync(file, Buffer.from(block.data, 'base64'));
    delete block.data;
    block.savedTo = file;
  }
  console.log(JSON.stringify(result, null, 2));
  if (result.isError) process.exitCode = 1;
} finally {
  clearTimeout(timer);
  lines.close();
  server.stdin.end();
  server.kill();
}
