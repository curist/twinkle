export const warmup = 10, iters = 40, size = 1000, expected = 499500;

function build(n) {
  let acc = null;
  for (let i = 0; i < n; i++) acc = { v: i, next: acc };
  return acc;
}

function sumList(xs) {
  let acc = 0;
  for (let cur = xs; cur !== null; cur = cur.next) acc += cur.v;
  return acc;
}

export function run(size) {
  const xs = build(size);
  return sumList(xs);
}
