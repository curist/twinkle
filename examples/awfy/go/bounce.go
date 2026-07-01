package main

// AWFY LCG, ported exactly.
func bounceRng(seed int) (int, int) {
	s := (seed*1309 + 13849) & 65535
	return s, s
}

type ball struct {
	x, y, xv, yv int
}

func bounceStep(b *ball) bool {
	const xLimit, yLimit = 500, 500
	bounced := false
	nx, ny, xv, yv := b.x+b.xv, b.y+b.yv, b.xv, b.yv
	if nx > xLimit {
		nx = xLimit
		xv = -xv
		bounced = true
	}
	if nx < 0 {
		nx = 0
		xv = -xv
		bounced = true
	}
	if ny > yLimit {
		ny = yLimit
		yv = -yv
		bounced = true
	}
	if ny < 0 {
		ny = 0
		yv = -yv
		bounced = true
	}
	b.x, b.y, b.xv, b.yv = nx, ny, xv, yv
	return bounced
}

func bounceRun(size int) int {
	const ballCount = 100
	total := 0
	for rep := 0; rep < size; rep++ {
		seed := 74755
		var v int
		balls := make([]ball, ballCount)
		for k := 0; k < ballCount; k++ {
			v, seed = bounceRng(seed)
			x := v % 500
			v, seed = bounceRng(seed)
			y := v % 500
			v, seed = bounceRng(seed)
			xv := (v % 5) - 2
			v, seed = bounceRng(seed)
			yv := (v % 5) - 2
			balls[k] = ball{x: x, y: y, xv: xv, yv: yv}
		}
		bounces := 0
		for step := 0; step < 50; step++ {
			for j := 0; j < ballCount; j++ {
				if bounceStep(&balls[j]) {
					bounces++
				}
			}
		}
		checksum := bounces
		for j := 0; j < ballCount; j++ {
			checksum += balls[j].x + balls[j].y
		}
		total = checksum
	}
	return total
}

var bounceBench = Bench{Name: "bounce", Warmup: 10, Iters: 20, Size: 400, Expected: 47174, Run: bounceRun}
