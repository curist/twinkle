export const warmup = 10, iters = 20, size = 120, expected = 27881;

function makeRng() {
  let seed = 74755;
  return () => { seed = (seed * 1309 + 13849) & 65535; return seed; };
}

export function run(size) {
  let result = 0;
  for (let rep = 0; rep < size; rep++) {
    const next = makeRng();
    let count = 0, leafElems = 0;
    const build = (depth) => {
      count++;
      if (depth === 1) {
        const n = (next() % 10) + 1;
        leafElems += n;
        return new Array(n).fill(null); // allocate a leaf array
      }
      const kids = new Array(4);
      for (let i = 0; i < 4; i++) kids[i] = build(depth - 1);
      return kids;
    };
    build(7);
    result = count + leafElems;
  }
  return result;
}
