package main

func queensGetRowColumn(freeRows, freeMaxs, freeMins []bool, r, c int) bool {
	return freeRows[r] && freeMaxs[c+r] && freeMins[c-r+7]
}

func queensSetRowColumn(freeRows, freeMaxs, freeMins []bool, r, c int, v bool) {
	freeRows[r] = v
	freeMaxs[c+r] = v
	freeMins[c-r+7] = v
}

func queensPlace(freeRows, freeMaxs, freeMins []bool, c int) bool {
	if c == 8 {
		return true
	}
	for r := 0; r < 8; r++ {
		if queensGetRowColumn(freeRows, freeMaxs, freeMins, r, c) {
			queensSetRowColumn(freeRows, freeMaxs, freeMins, r, c, false)
			if queensPlace(freeRows, freeMaxs, freeMins, c+1) {
				return true
			}
			queensSetRowColumn(freeRows, freeMaxs, freeMins, r, c, true)
		}
	}
	return false
}

func fillTrue(n int) []bool {
	s := make([]bool, n)
	for i := range s {
		s[i] = true
	}
	return s
}

func queensRun(size int) int {
	count := 0
	for n := 0; n < size; n++ {
		freeRows := fillTrue(8)
		freeMaxs := fillTrue(15)
		freeMins := fillTrue(15)
		if queensPlace(freeRows, freeMaxs, freeMins, 0) {
			count++
		}
	}
	return count
}

var queensBench = Bench{Name: "queens", Warmup: 10, Iters: 40, Size: 1000, Expected: 1000, Run: queensRun}
