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
		storageBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
