package main

import (
	"crypto/hmac"
	"crypto/md5"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/base64"
	"fmt"
	"time"
)

func makeBytes(n int) []byte {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte(i & 0xff)
	}
	return b
}

func printResult(bench string, iters int, elapsed time.Duration, sink byte) {
	fmt.Printf("go\t%s\t%d\t%f\t%d\n", bench, iters, float64(elapsed.Nanoseconds())/1e6, sink)
}

func main() {
	small := []byte("The quick brown fox jumps over the lazy dog")
	large := makeBytes(4096)
	key := []byte("key")

	itersSmall := 5001
	itersLarge := 501
	itersHmac := 3001
	itersB64 := 501

	var sink byte
	var start time.Time

	sink = 0
	start = time.Now()
	for i := 0; i < itersSmall; i++ {
		sum := md5.Sum(small)
		sink ^= sum[0]
	}
	printResult("md5_small", itersSmall, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersSmall; i++ {
		sum := sha1.Sum(small)
		sink ^= sum[0]
	}
	printResult("sha1_small", itersSmall, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersSmall; i++ {
		sum := sha256.Sum256(small)
		sink ^= sum[0]
	}
	printResult("sha256_small", itersSmall, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersHmac; i++ {
		h := hmac.New(sha256.New, key)
		h.Write(small)
		sink ^= h.Sum(nil)[0]
	}
	printResult("hmac_sha256_small", itersHmac, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersLarge; i++ {
		sum := md5.Sum(large)
		sink ^= sum[0]
	}
	printResult("md5_4k", itersLarge, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersLarge; i++ {
		sum := sha1.Sum(large)
		sink ^= sum[0]
	}
	printResult("sha1_4k", itersLarge, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersLarge; i++ {
		sum := sha256.Sum256(large)
		sink ^= sum[0]
	}
	printResult("sha256_4k", itersLarge, time.Since(start), sink)

	sink = 0
	start = time.Now()
	for i := 0; i < itersB64; i++ {
		text := base64.StdEncoding.EncodeToString(large)
		data, err := base64.StdEncoding.DecodeString(text)
		if err != nil {
			panic(err)
		}
		sink ^= data[0] ^ data[len(data)-1]
	}
	printResult("base64_roundtrip_4k", itersB64, time.Since(start), sink)
}
