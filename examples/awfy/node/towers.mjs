export const warmup = 10, iters = 40, size = 600, expected = 8191;

// Pegs are arrays used as stacks (top = last element). moveDisks mutates the
// three pegs in place and counts moves.
function moveTop(pegs, from, to) {
  const disk = pegs[from].pop();
  pegs[to].push(disk);
  return 1;
}

function moveDisks(pegs, n, from, to, via) {
  if (n === 1) return moveTop(pegs, from, to);
  let m = moveDisks(pegs, n - 1, from, via, to);
  m += moveTop(pegs, from, to);
  m += moveDisks(pegs, n - 1, via, to, from);
  return m;
}

export function run(size) {
  let moves = 0;
  for (let rep = 0; rep < size; rep++) {
    const a = [];
    for (let i = 0; i < 13; i++) a.push(13 - i); // largest at bottom
    const pegs = [a, [], []];
    moves = moveDisks(pegs, 13, 0, 2, 1);
  }
  return moves;
}
