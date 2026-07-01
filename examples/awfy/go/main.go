package main

func main() {
	benches := []Bench{
		smokeBench,
		mandelbrotBench,
		sieveBench,
		queensBench,
		permuteBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
