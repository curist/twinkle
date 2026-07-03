#!/usr/bin/env python3
import base64
import hashlib
import hmac
import time


def make_bytes(n):
    return bytes(i & 0xFF for i in range(n))


def print_result(bench, iters, elapsed_ms, sink):
    print(f"python\t{bench}\t{iters}\t{elapsed_ms}\t{sink}")


small = b"The quick brown fox jumps over the lazy dog"
large = make_bytes(4096)
key = b"key"

iters_small = 5001
iters_large = 501
iters_hmac = 3001
iters_b64 = 501

sink = 0
start = time.perf_counter()
for _ in range(iters_small):
    sink ^= hashlib.md5(small).digest()[0]
print_result("md5_small", iters_small, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_small):
    sink ^= hashlib.sha1(small).digest()[0]
print_result("sha1_small", iters_small, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_small):
    sink ^= hashlib.sha256(small).digest()[0]
print_result("sha256_small", iters_small, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_hmac):
    sink ^= hmac.new(key, small, hashlib.sha256).digest()[0]
print_result("hmac_sha256_small", iters_hmac, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_large):
    sink ^= hashlib.md5(large).digest()[0]
print_result("md5_4k", iters_large, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_large):
    sink ^= hashlib.sha1(large).digest()[0]
print_result("sha1_4k", iters_large, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_large):
    sink ^= hashlib.sha256(large).digest()[0]
print_result("sha256_4k", iters_large, (time.perf_counter() - start) * 1000, sink)

sink = 0
start = time.perf_counter()
for _ in range(iters_b64):
    text = base64.b64encode(large)
    data = base64.b64decode(text)
    sink ^= data[0] ^ data[-1]
print_result("base64_roundtrip_4k", iters_b64, (time.perf_counter() - start) * 1000, sink)
