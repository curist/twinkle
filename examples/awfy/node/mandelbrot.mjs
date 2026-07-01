export const warmup = 10, iters = 20, size = 500, expected = 191;

export function run(size) {
  let sum = 0, byteAcc = 0, bitNum = 0, y = 0;
  while (y < size) {
    const ci = (2.0 * y / size) - 1.0;
    let x = 0;
    while (x < size) {
      let zr = 0.0, zrzr = 0.0, zi = 0.0, zizi = 0.0;
      const cr = (2.0 * x / size) - 1.5;
      let z = 0, notDone = true, escape = 0;
      while (notDone && z < 50) {
        zr = zrzr - zizi + cr;
        zi = 2.0 * zr * zi + ci;
        zrzr = zr * zr;
        zizi = zi * zi;
        if (zrzr + zizi > 4.0) { notDone = false; escape = 1; }
        z += 1;
      }
      byteAcc = (byteAcc << 1) + escape;
      bitNum += 1;
      if (bitNum === 8) { sum ^= byteAcc; byteAcc = 0; bitNum = 0; }
      else if (x === size - 1) { byteAcc <<= (8 - bitNum); sum ^= byteAcc; byteAcc = 0; bitNum = 0; }
      x += 1;
    }
    y += 1;
  }
  return sum;
}
