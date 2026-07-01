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
		bounceBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
