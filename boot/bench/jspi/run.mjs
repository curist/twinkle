// Phase A JSPI switching-cost harness (throwaway bench).
// See docs/plans/task-concurrency-jspi-fiber.md, section "Spike".
//
// Runs boot/bench/jspi/phase_a.wasm through the SAME async runtime path as
// `twk run`, providing the `bench` externs ourselves so we control whether each
// suspends. Times are taken on the JS host via performance.now() inside the
// `mark` extern. Reports per-iteration cost for A1/A2/A3 plus the A3/A2 ratio
// (engine fast-path detection for already-resolved promises).
//
// Usage (run on each target runtime for the cross-runtime step):
//   node boot/bench/jspi/run.mjs
//   deno run -A boot/bench/jspi/run.mjs

import { readFileSync } from "node:fs";
import { runWasmBytesAsync, hasJspi } from "../../../tools/js_runtime/runtime.mjs";
import { nodeHost } from "../../../tools/js_runtime/node_host.mjs";

const here = import.meta.dirname;
const N = 100000; // must match `n` in phase_a.tw
const WARMUP = 2;
const RUNS = 7;

if (!hasJspi) {
  console.error("This runtime lacks WebAssembly.Suspending/promising (JSPI). Aborting.");
  globalThis.process?.exit?.(1);
}

const wasmBytes = readFileSync(`${here}/phase_a.wasm`);
const bridgeBytes = readFileSync(`${here}/../../../tools/bridge.wasm`);

// Recorded by the `mark` extern, in call order: [t0, t1, t2, t3].
let marks = [];

const imports = {
  bench: {
    mark: () => { marks.push(performance.now()); },
    // A2: already-resolved promise -> engine may fast-path (no real switch).
    suspend_resolved: async () => {},
    // A3: deferred one microtask -> forces a real suspend + resume.
    suspend_micro: async () => { await Promise.resolve(); },
    // Keep `acc` live so the baseline loop is not dead-code-eliminated.
    sink: () => {},
  },
};

async function oneRun() {
  marks = [];
  await runWasmBytesAsync(wasmBytes, {
    programPath: "phase_a.wasm",
    guestArgs: [],
    cwd: globalThis.process?.cwd?.() ?? ".",
    env: globalThis.process?.env ?? {},
    stdout: globalThis.process?.stdout,
    stderr: globalThis.process?.stderr,
    bridgeBytes,
    host: nodeHost,
    imports,
  });
  if (marks.length !== 4) throw new Error(`expected 4 marks, got ${marks.length}`);
  return {
    a1: marks[1] - marks[0],
    a2: marks[2] - marks[1],
    a3: marks[3] - marks[2],
  };
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};
const usPerIter = (ms) => (ms * 1000) / N; // ms-total -> us-per-iteration

const runtimeName =
  typeof globalThis.Deno !== "undefined"
    ? `Deno ${globalThis.Deno.version.deno}`
    : `Node ${globalThis.process?.versions?.node}`;

for (let i = 0; i < WARMUP; i++) await oneRun();
const samples = [];
for (let i = 0; i < RUNS; i++) samples.push(await oneRun());

const a1 = median(samples.map((s) => s.a1));
const a2 = median(samples.map((s) => s.a2));
const a3 = median(samples.map((s) => s.a3));

console.log(`\nPhase A — JSPI switching cost   [${runtimeName}]`);
console.log(`N=${N} per loop, ${RUNS} runs (median), ${WARMUP} warmup\n`);
const row = (label, ms) =>
  console.log(`  ${label.padEnd(34)} ${usPerIter(ms).toFixed(4).padStart(9)} us/iter   (${ms.toFixed(2)} ms total)`);
row("A1 baseline loop (no suspend)", a1);
row("A2 suspend, already-resolved", a2);
row("A3 suspend, microtask (real)", a3);
console.log("");
console.log(`  A3 / A2 ratio (fast-path tell):  ${(a3 / a2).toFixed(2)}x`);
console.log(`  A2 - A1 (resolved suspend cost): ${usPerIter(a2 - a1).toFixed(4)} us/iter`);
console.log(`  A3 - A1 (real suspend cost):     ${usPerIter(a3 - a1).toFixed(4)} us/iter`);
console.log("");
