package main

type listNode struct {
	v    int
	next *listNode
}

func listBuild(n int) *listNode {
	var acc *listNode
	for i := 0; i < n; i++ {
		acc = &listNode{v: i, next: acc}
	}
	return acc
}

func listSum(xs *listNode) int {
	acc := 0
	for cur := xs; cur != nil; cur = cur.next {
		acc += cur.v
	}
	return acc
}

func listRun(size int) int {
	return listSum(listBuild(size))
}

var listBench = Bench{Name: "list", Warmup: 10, Iters: 40, Size: 1000, Expected: 499500, Run: listRun}
