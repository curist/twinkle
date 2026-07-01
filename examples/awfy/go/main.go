package main

func main() {
	benches := []Bench{
		smokeBench,
		mandelbrotBench,
		sieveBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
