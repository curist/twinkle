export const warmup = 10, iters = 40, size = 5000, expected = 669;

export function run(size) {
  const flags = new Array(size + 1).fill(true);
  let count = 0;
  for (let i = 2; i <= size; i++) {
    if (flags[i]) {
      count++;
      for (let k = i + i; k <= size; k += i) flags[k] = false;
    }
  }
  return count;
}
