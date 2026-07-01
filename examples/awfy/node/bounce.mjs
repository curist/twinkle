export const warmup = 10, iters = 20, size = 400, expected = 47174;

// AWFY LCG, ported exactly.
function makeRng() {
  let seed = 74755;
  return () => { seed = (seed * 1309 + 13849) & 65535; return seed; };
}

function newBall(next) {
  return {
    x: next() % 500,
    y: next() % 500,
    xv: (next() % 5) - 2,
    yv: (next() % 5) - 2,
  };
}

function bounceStep(b) {
  const xLimit = 500, yLimit = 500;
  let bounced = false;
  let nx = b.x + b.xv, ny = b.y + b.yv, xv = b.xv, yv = b.yv;
  if (nx > xLimit) { nx = xLimit; xv = -xv; bounced = true; }
  if (nx < 0) { nx = 0; xv = -xv; bounced = true; }
  if (ny > yLimit) { ny = yLimit; yv = -yv; bounced = true; }
  if (ny < 0) { ny = 0; yv = -yv; bounced = true; }
  b.x = nx; b.y = ny; b.xv = xv; b.yv = yv;
  return bounced;
}

export function run(size) {
  const ballCount = 100;
  let total = 0;
  for (let rep = 0; rep < size; rep++) {
    const next = makeRng();
    const balls = [];
    for (let k = 0; k < ballCount; k++) balls.push(newBall(next));
    let bounces = 0;
    for (let step = 0; step < 50; step++) {
      for (let j = 0; j < ballCount; j++) {
        if (bounceStep(balls[j])) bounces++;
      }
    }
    let checksum = bounces;
    for (const b of balls) checksum += b.x + b.y;
    total = checksum;
  }
  return total;
}
