// Phase B JSPI scheduler benchmark harness (throwaway bench).
// See docs/plans/task-concurrency-jspi-fiber.md, section "Spike".
//
// Runs boot/bench/jspi/phase_b.wasm through the SAME async runtime path as
// `twk run` (so the real cooperative scheduler drives the Task.* lowering),
// providing the `bench` externs ourselves to time interval boundaries on the
// JS host via performance.now(). Reports per-operation cost for B1–B4.
//
// Build the wasm first, then run on each target runtime:
//   target/twk build boot/bench/jspi/phase_b.tw -o boot/bench/jspi/phase_b.wasm
//   node boot/bench/jspi/run_b.mjs
//   deno run -A boot/bench/jspi/run_b.mjs

import { readFileSync } from "node:fs";
import { runWasmBytesAsync, hasJspi } from "../../../tools/js_runtime/runtime.mjs";
import { nodeHost } from "../../../tools/js_runtime/node_host.mjs";

const here = import.meta.dirname;
// Must match the counts in phase_b.tw.
const YIELDS = 100000;
const SPAWN_AWAITS = 50000;
const SLEEPS = 200;
const REQUESTS = 20000;
const WARMUP = 2;
const RUNS = 5;

if (!hasJspi) {
  console.error("This runtime lacks WebAssembly.Suspending/promising (JSPI). Aborting.");
  globalThis.process?.exit?.(1);
}

const wasmBytes = readFileSync(`${here}/phase_b.wasm`);
const bridgeBytes = readFileSync(`${here}/../../../tools/bridge.wasm`);

// Recorded by the `mark` extern, in call order: [t0, t1, t2, t3, t4].
let marks = [];

const imports = {
  bench: {
    mark: () => { marks.push(performance.now()); },
    sink: () => {},
  },
};

async function oneRun() {
  marks = [];
  await runWasmBytesAsync(wasmBytes, {
    programPath: "phase_b.wasm",
    guestArgs: [],
    cwd: globalThis.process?.cwd?.() ?? ".",
    env: globalThis.process?.env ?? {},
    stdout: globalThis.process?.stdout,
    stderr: globalThis.process?.stderr,
    bridgeBytes,
    host: nodeHost,
    imports,
  });
  if (marks.length !== 5) throw new Error(`expected 5 marks, got ${marks.length}`);
  return {
    b1: marks[1] - marks[0],
    b2: marks[2] - marks[1],
    b3: marks[3] - marks[2],
    b4: marks[4] - marks[3],
  };
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};
const usPer = (ms, n) => (ms * 1000) / n; // ms-total -> us-per-operation

const runtimeName =
  typeof globalThis.Deno !== "undefined"
    ? `Deno ${globalThis.Deno.version.deno}`
    : `Node ${globalThis.process?.versions?.node}`;

for (let i = 0; i < WARMUP; i++) await oneRun();
const samples = [];
for (let i = 0; i < RUNS; i++) samples.push(await oneRun());

const b1 = median(samples.map((s) => s.b1));
const b2 = median(samples.map((s) => s.b2));
const b3 = median(samples.map((s) => s.b3));
const b4 = median(samples.map((s) => s.b4));

console.log(`\nPhase B — JSPI scheduler cost   [${runtimeName}]`);
console.log(`${RUNS} runs (median), ${WARMUP} warmup\n`);
const row = (label, ms, n) =>
  console.log(
    `  ${label.padEnd(40)} ${usPer(ms, n).toFixed(3).padStart(9)} us/op   (${ms.toFixed(1).padStart(8)} ms / ${n})`,
  );
row(`B1 Task.yield() round-trip`, b1, YIELDS);
row(`B2 spawn + await round-trip`, b2, SPAWN_AWAITS);
row(`B3 time.sleep(0) (timer floor, not switch)`, b3, SLEEPS);
row(`B4 LSP-shaped dispatch (spawn+yield+await)`, b4, REQUESTS);
console.log("");
