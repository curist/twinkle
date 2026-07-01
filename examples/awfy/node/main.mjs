import { runBench } from "./harness.mjs";
import * as smoke from "./smoke.mjs";

const benches = [
  { name: "smoke", warmup: smoke.warmup, iters: smoke.iters, size: smoke.size, expected: smoke.expected, run: smoke.run },
];
for (const b of benches) runBench(b);
