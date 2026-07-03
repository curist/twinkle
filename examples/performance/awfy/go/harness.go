package main

import (
	"fmt"
	"time"
)

type Bench struct {
	Name     string
	Warmup   int
	Iters    int
	Size     int
	Expected int
	Run      func(int) int
}

func RunBench(b Bench) {
	warmSink := 0
	for w := 0; w < b.Warmup; w++ {
		warmSink ^= b.Run(b.Size)
	}
	if warmSink == 0x7fffffff {
		fmt.Println("unreachable", warmSink)
	}

	start := time.Now()
	checksum := 0
	for i := 0; i < b.Iters; i++ {
		checksum = b.Run(b.Size)
	}
	elapsedMs := float64(time.Since(start).Nanoseconds()) / 1e6

	if checksum != b.Expected {
		panic(fmt.Sprintf("%s: checksum %d != expected %d", b.Name, checksum, b.Expected))
	}
	fmt.Printf("go\t%s\t%d\t%g\t%d\n", b.Name, b.Iters, elapsedMs, checksum)
}
