export const warmup = 1, iters = 1, size = 3, expected = 6;
export function run(size) {
  let sum = 0;
  for (let i = 1; i <= size; i++) sum += i;
  return sum;
}
