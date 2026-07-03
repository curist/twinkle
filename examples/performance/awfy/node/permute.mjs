export const warmup = 10, iters = 20, size = 300, expected = 8660;

let v;
let count;

function permute(n) {
  count++;
  if (n !== 0) {
    permute(n - 1);
    for (let i = n - 1; i >= 0; i--) {
      swap(n - 1, i);
      permute(n - 1);
      swap(n - 1, i);
    }
  }
}

function swap(i, j) {
  const tmp = v[i];
  v[i] = v[j];
  v[j] = tmp;
}

export function run(size) {
  for (let rep = 0; rep < size; rep++) {
    count = 0;
    v = new Array(6).fill(0);
    permute(6);
  }
  return count;
}
