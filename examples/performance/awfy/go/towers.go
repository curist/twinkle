package main

// Pegs are slices used as stacks (top = last element). moveDisks mutates the
// three pegs in place and counts moves.
func towersMoveTop(pegs [][]int, from, to int) int {
	n := len(pegs[from])
	disk := pegs[from][n-1]
	pegs[from] = pegs[from][:n-1]
	pegs[to] = append(pegs[to], disk)
	return 1
}

func towersMoveDisks(pegs [][]int, n, from, to, via int) int {
	if n == 1 {
		return towersMoveTop(pegs, from, to)
	}
	m := towersMoveDisks(pegs, n-1, from, via, to)
	m += towersMoveTop(pegs, from, to)
	m += towersMoveDisks(pegs, n-1, via, to, from)
	return m
}

func towersRun(size int) int {
	moves := 0
	for rep := 0; rep < size; rep++ {
		a := make([]int, 0, 13)
		for i := 0; i < 13; i++ {
			a = append(a, 13-i) // largest at bottom
		}
		pegs := [][]int{a, {}, {}}
		moves = towersMoveDisks(pegs, 13, 0, 2, 1)
	}
	return moves
}

var towersBench = Bench{Name: "towers", Warmup: 10, Iters: 20, Size: 200, Expected: 8191, Run: towersRun}
