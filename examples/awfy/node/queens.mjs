export const warmup = 10, iters = 40, size = 1000, expected = 1000;

function getRowColumn(freeRows, freeMaxs, freeMins, r, c) {
  return freeRows[r] && freeMaxs[c + r] && freeMins[c - r + 7];
}

function setRowColumn(freeRows, freeMaxs, freeMins, r, c, v) {
  freeRows[r] = v;
  freeMaxs[c + r] = v;
  freeMins[c - r + 7] = v;
}

function place(freeRows, freeMaxs, freeMins, c) {
  if (c === 8) return true;
  for (let r = 0; r < 8; r++) {
    if (getRowColumn(freeRows, freeMaxs, freeMins, r, c)) {
      setRowColumn(freeRows, freeMaxs, freeMins, r, c, false);
      if (place(freeRows, freeMaxs, freeMins, c + 1)) return true;
      setRowColumn(freeRows, freeMaxs, freeMins, r, c, true);
    }
  }
  return false;
}

export function run(size) {
  let count = 0;
  for (let n = 0; n < size; n++) {
    const freeRows = new Array(8).fill(true);
    const freeMaxs = new Array(15).fill(true);
    const freeMins = new Array(15).fill(true);
    if (place(freeRows, freeMaxs, freeMins, 0)) count++;
  }
  return count;
}
