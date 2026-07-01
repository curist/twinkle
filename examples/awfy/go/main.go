package main

func main() {
	benches := []Bench{
		smokeBench,
		mandelbrotBench,
		sieveBench,
		queensBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
