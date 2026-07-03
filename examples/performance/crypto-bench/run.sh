#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../../.."

printf 'lang\tbench\titers\tms\tsink\tus_per_op\n'

normalize() {
  awk -F '\t' 'NF >= 5 { printf "%s\t%s\t%s\t%s\t%s\t%.6f\n", $1, $2, $3, $4, $5, ($4 * 1000.0) / $3 }'
}

target/twk run examples/performance/crypto-bench/twinkle/main.tw | normalize
node examples/performance/crypto-bench/node.mjs | normalize
python3 examples/performance/crypto-bench/python.py | normalize
go run examples/performance/crypto-bench/go.go | normalize
