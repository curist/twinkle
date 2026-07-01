package main

// Storage builds a nested tree of slices to stress the allocator/GC. The RNG
// (shared AWFY LCG) only sets leaf sizes; node count is fixed by the fanout.
func storageRun(size int) int {
	result := 0
	for rep := 0; rep < size; rep++ {
		seed := 74755
		next := func() int { seed = (seed*1309 + 13849) & 65535; return seed }
		count, leafElems := 0, 0
		var build func(depth int) []interface{}
		build = func(depth int) []interface{} {
			count++
			if depth == 1 {
				n := (next() % 10) + 1
				leafElems += n
				return make([]interface{}, n)
			}
			kids := make([]interface{}, 4)
			for i := 0; i < 4; i++ {
				kids[i] = build(depth - 1)
			}
			return kids
		}
		build(7)
		result = count + leafElems
	}
	return result
}

var storageBench = Bench{Name: "storage", Warmup: 10, Iters: 20, Size: 120, Expected: 27881, Run: storageRun}
