#!/usr/bin/env node
import { spawn } from "node:child_process";
import { once } from "node:events";
import process from "node:process";

const cli = process.argv[2] ?? "target/twk";
const uri = "file:///tmp/twinkle-lsp-smoke.tw";

function frame(message) {
  const body = Buffer.from(JSON.stringify(message), "utf8");
  return Buffer.concat([
    Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "utf8"),
    body,
  ]);
}

function decodeFrames(buffer) {
  const messages = [];
  let rest = buffer;

  while (true) {
    const marker = rest.indexOf("\r\n\r\n");
    if (marker < 0) break;

    const header = rest.subarray(0, marker).toString("ascii");
    const match = /^Content-Length: (\d+)$/im.exec(header);
    if (!match) throw new Error(`missing Content-Length header: ${header}`);

    const start = marker + 4;
    const length = Number(match[1]);
    const end = start + length;
    if (rest.length < end) break;

    messages.push(JSON.parse(rest.subarray(start, end).toString("utf8")));
    rest = rest.subarray(end);
  }

  return { messages, rest };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function diagnosticsFor(messages, version) {
  return messages.find((message) =>
    message.method === "textDocument/publishDiagnostics" &&
    message.params?.uri === uri &&
    message.params?.version === version
  );
}

const child = spawn(cli, ["lsp"], { stdio: ["pipe", "pipe", "pipe"] });
let stdout = Buffer.alloc(0);
let stderr = "";
const messages = [];

child.stdout.on("data", (chunk) => {
  stdout = Buffer.concat([stdout, chunk]);
  const decoded = decodeFrames(stdout);
  messages.push(...decoded.messages);
  stdout = decoded.rest;
});
child.stderr.on("data", (chunk) => {
  stderr += chunk.toString("utf8");
});

const requests = [
  { jsonrpc: "2.0", id: 1, method: "initialize", params: {} },
  { jsonrpc: "2.0", method: "initialized", params: {} },
  {
    jsonrpc: "2.0",
    method: "textDocument/didOpen",
    params: {
      textDocument: {
        uri,
        languageId: "twinkle",
        version: 1,
        text: "x := 1\n",
      },
    },
  },
  {
    jsonrpc: "2.0",
    method: "textDocument/didChange",
    params: {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: "x := \n" }],
    },
  },
  {
    jsonrpc: "2.0",
    method: "textDocument/didClose",
    params: { textDocument: { uri } },
  },
  { jsonrpc: "2.0", id: 2, method: "shutdown", params: null },
  { jsonrpc: "2.0", method: "exit", params: null },
];

for (const request of requests) child.stdin.write(frame(request));
child.stdin.end();

const [code, signal] = await once(child, "exit");
const tail = decodeFrames(stdout);
messages.push(...tail.messages);
stdout = tail.rest;

try {
  assert(code === 0 && signal === null, `LSP exited with code ${code} signal ${signal}\n${stderr}`);
  assert(stdout.length === 0, `leftover stdout bytes after decoding frames: ${stdout.length}`);
  assert(stderr.trim() === "", `unexpected stderr:\n${stderr}`);

  const initialize = messages.find((message) => message.id === 1);
  assert(initialize?.result?.capabilities?.textDocumentSync === 1, "initialize response missing text sync capability");

  const opened = diagnosticsFor(messages, 1);
  assert(opened, "didOpen did not publish diagnostics");
  assert(opened.params.diagnostics.length === 0, "valid didOpen text should publish empty diagnostics");

  const changed = diagnosticsFor(messages, 2);
  assert(changed, "didChange did not publish diagnostics");
  assert(changed.params.diagnostics.length > 0, "invalid didChange text should publish diagnostics");

  const clears = messages.filter((message) =>
    message.method === "textDocument/publishDiagnostics" &&
    message.params?.uri === uri &&
    message.params?.diagnostics?.length === 0
  );
  assert(clears.length >= 2, "didClose should publish an empty diagnostics notification");

  const shutdown = messages.find((message) => message.id === 2);
  assert(shutdown && Object.hasOwn(shutdown, "result") && shutdown.result === null, "shutdown response should be null");

  console.log("LSP framed stdio smoke passed");
} catch (error) {
  console.error(error.message);
  console.error(JSON.stringify(messages, null, 2));
  process.exit(1);
}
