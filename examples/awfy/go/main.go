package main

func main() {
	benches := []Bench{
		smokeBench,
		mandelbrotBench,
		sieveBench,
		queensBench,
		permuteBench,
		towersBench,
		listBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
