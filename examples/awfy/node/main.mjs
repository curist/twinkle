import { runBench } from "./harness.mjs";
import * as smoke from "./smoke.mjs";
import * as mandelbrot from "./mandelbrot.mjs";

const benches = [
  { name: "smoke", warmup: smoke.warmup, iters: smoke.iters, size: smoke.size, expected: smoke.expected, run: smoke.run },
  { name: "mandelbrot", warmup: mandelbrot.warmup, iters: mandelbrot.iters, size: mandelbrot.size, expected: mandelbrot.expected, run: mandelbrot.run },
];
for (const b of benches) runBench(b);
