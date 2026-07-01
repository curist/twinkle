package main

func smokeRun(size int) int {
	sum := 0
	for i := 1; i <= size; i++ {
		sum += i
	}
	return sum
}

var smokeBench = Bench{Name: "smoke", Warmup: 1, Iters: 1, Size: 3, Expected: 6, Run: smokeRun}
