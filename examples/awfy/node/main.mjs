import { runBench } from "./harness.mjs";
import * as smoke from "./smoke.mjs";
import * as mandelbrot from "./mandelbrot.mjs";
import * as sieve from "./sieve.mjs";
import * as queens from "./queens.mjs";
import * as permute from "./permute.mjs";
import * as towers from "./towers.mjs";
import * as list from "./list.mjs";
import * as bounce from "./bounce.mjs";
import * as storage from "./storage.mjs";

const benches = [
  { name: "smoke", warmup: smoke.warmup, iters: smoke.iters, size: smoke.size, expected: smoke.expected, run: smoke.run },
  { name: "mandelbrot", warmup: mandelbrot.warmup, iters: mandelbrot.iters, size: mandelbrot.size, expected: mandelbrot.expected, run: mandelbrot.run },
  { name: "sieve", warmup: sieve.warmup, iters: sieve.iters, size: sieve.size, expected: sieve.expected, run: sieve.run },
  { name: "queens", warmup: queens.warmup, iters: queens.iters, size: queens.size, expected: queens.expected, run: queens.run },
  { name: "permute", warmup: permute.warmup, iters: permute.iters, size: permute.size, expected: permute.expected, run: permute.run },
  { name: "towers", warmup: towers.warmup, iters: towers.iters, size: towers.size, expected: towers.expected, run: towers.run },
  { name: "list", warmup: list.warmup, iters: list.iters, size: list.size, expected: list.expected, run: list.run },
  { name: "bounce", warmup: bounce.warmup, iters: bounce.iters, size: bounce.size, expected: bounce.expected, run: bounce.run },
  { name: "storage", warmup: storage.warmup, iters: storage.iters, size: storage.size, expected: storage.expected, run: storage.run },
];
for (const b of benches) runBench(b);
