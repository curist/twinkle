package main

func main() {
	benches := []Bench{
		smokeBench,
		mandelbrotBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
