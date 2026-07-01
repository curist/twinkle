package main

func sieveRun(size int) int {
	flags := make([]bool, size+1)
	for i := range flags {
		flags[i] = true
	}
	count := 0
	for i := 2; i <= size; i++ {
		if flags[i] {
			count++
			for k := i + i; k <= size; k += i {
				flags[k] = false
			}
		}
	}
	return count
}

var sieveBench = Bench{Name: "sieve", Warmup: 10, Iters: 40, Size: 5000, Expected: 669, Run: sieveRun}
