// Native Go comparison for the dataframe order_by("amount", Asc) path.
//
// Run from repository root:
//
//	go run examples/dataframe/bench/order_by_go.go
//
// Mirrors examples/dataframe/bench/main.tw for order_by only:
// generate key/amount/score columns, sort row indices by amount, gather all columns.
package main

import (
	"fmt"
	"sort"
	"time"
)

type Table struct {
	keys    []string
	amounts []int64
	scores  []float64
}

func nextSeed(seed int64) int64 {
	return (seed*1664525 + 1013904223) % 2147483648
}

func genTable(n int, keyCardinality int64) Table {
	keys := make([]string, 0, n)
	amounts := make([]int64, 0, n)
	scores := make([]float64, 0, n)
	seed := int64(12345)

	for i := 0; i < n; i++ {
		seed = nextSeed(seed)
		keys = append(keys, fmt.Sprintf("k%d", seed%keyCardinality))

		seed = nextSeed(seed)
		amounts = append(amounts, seed%1000)

		seed = nextSeed(seed)
		scores = append(scores, float64(seed%10000)/100.0)
	}

	return Table{keys: keys, amounts: amounts, scores: scores}
}

func gather(t Table, idx []int) Table {
	keys := make([]string, 0, len(idx))
	amounts := make([]int64, 0, len(idx))
	scores := make([]float64, 0, len(idx))

	for _, row := range idx {
		keys = append(keys, t.keys[row])
		amounts = append(amounts, t.amounts[row])
		scores = append(scores, t.scores[row])
	}

	return Table{keys: keys, amounts: amounts, scores: scores}
}

func orderByAmountAsc(t Table) (Table, time.Duration, time.Duration) {
	idx := make([]int, len(t.amounts))
	for i := range idx {
		idx[i] = i
	}

	sortStart := time.Now()
	sort.Slice(idx, func(i, j int) bool {
		return t.amounts[idx[i]] < t.amounts[idx[j]]
	})
	sortElapsed := time.Since(sortStart)

	gatherStart := time.Now()
	out := gather(t, idx)
	gatherElapsed := time.Since(gatherStart)

	return out, sortElapsed, gatherElapsed
}

func ms(d time.Duration) float64 {
	return float64(d.Nanoseconds()) / 1_000_000.0
}

func benchN(n int) {
	base := genTable(n, 64)

	start := time.Now()
	sorted, sortElapsed, gatherElapsed := orderByAmountAsc(base)
	total := time.Since(start)

	fmt.Printf("N=%-8d go total: %8.2fms  sort: %8.2fms  gather: %8.2fms  checksum %d\n",
		n, ms(total), ms(sortElapsed), ms(gatherElapsed), len(sorted.amounts))
}

func main() {
	for _, n := range []int{10000, 100000, 1000000} {
		benchN(n)
	}
}
