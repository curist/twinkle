#!/usr/bin/env node
import { spawn } from "node:child_process";
import { once } from "node:events";
import process from "node:process";

const cli = process.argv[2] ?? "target/twk";
const uri = "file:///tmp/twinkle-lsp-smoke.tw";
const importerUri = "file:///tmp/twinkle-lsp-smoke/main.tw";
const depUri = "file:///tmp/twinkle-lsp-smoke/dep.tw";

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

function diagnosticsFor(messages, targetUri, version) {
  return messages.find((message) =>
    message.method === "textDocument/publishDiagnostics" &&
    message.params?.uri === targetUri &&
    message.params?.version === version
  );
}

function diagnosticEvents(messages, targetUri, version) {
  return messages.filter((message) =>
    message.method === "textDocument/publishDiagnostics" &&
    message.params?.uri === targetUri &&
    (version === undefined || message.params?.version === version)
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

function didOpen(targetUri, version, text) {
  return {
    jsonrpc: "2.0",
    method: "textDocument/didOpen",
    params: {
      textDocument: {
        uri: targetUri,
        languageId: "twinkle",
        version,
        text,
      },
    },
  };
}

function didChange(targetUri, version, text) {
  return {
    jsonrpc: "2.0",
    method: "textDocument/didChange",
    params: {
      textDocument: { uri: targetUri, version },
      contentChanges: [{ text }],
    },
  };
}

function didClose(targetUri) {
  return {
    jsonrpc: "2.0",
    method: "textDocument/didClose",
    params: { textDocument: { uri: targetUri } },
  };
}

const requests = [
  { jsonrpc: "2.0", id: 1, method: "initialize", params: {} },
  { jsonrpc: "2.0", method: "initialized", params: {} },
  didOpen(uri, 1, "x := 1\n"),
  didChange(uri, 2, "x := \n"),
  didClose(uri),
  didOpen(depUri, 1, "pub fn answer() Int { 1 }\n"),
  didOpen(importerUri, 1, "use dep\nx: Int = dep.answer()\n"),
  didChange(depUri, 2, "pub fn answer() String { \"nope\" }\n"),
  didChange(depUri, 3, "pub fn answer() Int { 1 }\n"),
  didClose(importerUri),
  didClose(depUri),
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

  const opened = diagnosticsFor(messages, uri, 1);
  assert(opened, "didOpen did not publish diagnostics");
  assert(opened.params.diagnostics.length === 0, "valid didOpen text should publish empty diagnostics");

  const changed = diagnosticsFor(messages, uri, 2);
  assert(changed, "didChange did not publish diagnostics");
  assert(changed.params.diagnostics.length > 0, "invalid didChange text should publish diagnostics");

  const clears = diagnosticEvents(messages, uri).filter((message) =>
    message.params?.diagnostics?.length === 0
  );
  assert(clears.length >= 2, "didClose should publish an empty diagnostics notification");

  const depOpened = diagnosticsFor(messages, depUri, 1);
  assert(depOpened, "dependency didOpen did not publish diagnostics");
  assert(depOpened.params.diagnostics.length === 0, "valid dependency should publish empty diagnostics");

  const importerEvents = diagnosticEvents(messages, importerUri, 1);
  assert(importerEvents.some((message) => message.params.diagnostics.length > 0),
    "dependency edit should publish importer diagnostics");
  assert(importerEvents.some((message) => message.params.diagnostics.length === 0),
    "dependency fix should clear importer diagnostics");

  const badDep = diagnosticsFor(messages, depUri, 2);
  assert(badDep, "dependency edit did not publish dependency diagnostics");
  assert(badDep.params.diagnostics.length === 0, "well-typed dependency edit should not diagnose dependency itself");

  const fixedDep = diagnosticsFor(messages, depUri, 3);
  assert(fixedDep, "dependency fix did not publish dependency diagnostics");
  assert(fixedDep.params.diagnostics.length === 0, "fixed dependency should publish empty diagnostics");

  const shutdown = messages.find((message) => message.id === 2);
  assert(shutdown && Object.hasOwn(shutdown, "result") && shutdown.result === null, "shutdown response should be null");

  console.log("LSP framed stdio smoke passed");
} catch (error) {
  console.error(error.message);
  console.error(JSON.stringify(messages, null, 2));
  process.exit(1);
}
