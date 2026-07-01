package main

func main() {
	benches := []Bench{
		smokeBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
