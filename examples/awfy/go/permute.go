package main

var permuteV []int
var permuteCount int

func permuteSwap(i, j int) {
	permuteV[i], permuteV[j] = permuteV[j], permuteV[i]
}

func permuteRec(n int) {
	permuteCount++
	if n != 0 {
		permuteRec(n - 1)
		for i := n - 1; i >= 0; i-- {
			permuteSwap(n-1, i)
			permuteRec(n - 1)
			permuteSwap(n-1, i)
		}
	}
}

func permuteRun(size int) int {
	for rep := 0; rep < size; rep++ {
		permuteCount = 0
		permuteV = make([]int, 6)
		permuteRec(6)
	}
	return permuteCount
}

var permuteBench = Bench{Name: "permute", Warmup: 10, Iters: 40, Size: 1000, Expected: 8660, Run: permuteRun}
