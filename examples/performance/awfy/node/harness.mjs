export function runBench(b) {
  let warmSink = 0;
  for (let w = 0; w < b.warmup; w++) warmSink ^= b.run(b.size);
  if (warmSink === 0x7fffffff) console.log("unreachable", warmSink);

  const start = performance.now();
  let checksum = 0;
  for (let i = 0; i < b.iters; i++) checksum = b.run(b.size);
  const elapsed = performance.now() - start;

  if (checksum !== b.expected) {
    throw new Error(`${b.name}: checksum ${checksum} != expected ${b.expected}`);
  }
  console.log(`node\t${b.name}\t${b.iters}\t${elapsed}\t${checksum}`);
}
