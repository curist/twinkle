package main

func mandelbrotRun(size int) int {
	sum, byteAcc, bitNum, y := 0, 0, 0, 0
	fsize := float64(size)
	for y < size {
		ci := (2.0*float64(y)/fsize) - 1.0
		x := 0
		for x < size {
			zr, zrzr, zi, zizi := 0.0, 0.0, 0.0, 0.0
			cr := (2.0*float64(x)/fsize) - 1.5
			z, notDone, escape := 0, true, 0
			for notDone && z < 50 {
				zr = zrzr - zizi + cr
				zi = 2.0*zr*zi + ci
				zrzr = zr * zr
				zizi = zi * zi
				if zrzr+zizi > 4.0 {
					notDone = false
					escape = 1
				}
				z++
			}
			byteAcc = (byteAcc << 1) + escape
			bitNum++
			if bitNum == 8 {
				sum ^= byteAcc
				byteAcc, bitNum = 0, 0
			} else if x == size-1 {
				byteAcc <<= (8 - bitNum)
				sum ^= byteAcc
				byteAcc, bitNum = 0, 0
			}
			x++
		}
		y++
	}
	return sum
}

var mandelbrotBench = Bench{Name: "mandelbrot", Warmup: 10, Iters: 30, Size: 500, Expected: 191, Run: mandelbrotRun}
