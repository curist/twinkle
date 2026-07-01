#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

raw="$(mktemp)"
trap 'rm -f "$raw"' EXIT

target/twk run examples/awfy/twinkle/main.tw >> "$raw"
node examples/awfy/node/main.mjs             >> "$raw"
(cd examples/awfy/go && go run .)            >> "$raw"

# Checksum agreement: for each bench, all langs must report the same checksum.
mismatch="$(awk -F '\t' '
  NF >= 5 {
    key = $2
    if (key in seen) {
      if (sum[key] != $5) bad[key] = 1
    } else {
      seen[key] = 1; sum[key] = $5
    }
  }
  END { for (k in bad) print k, "checksums disagree" }
' "$raw")"

if [ -n "$mismatch" ]; then
  echo "CHECKSUM MISMATCH:" >&2
  echo "$mismatch" >&2
  echo "--- raw rows ---" >&2
  sort -t$'\t' -k2,2 "$raw" >&2
  exit 1
fi

printf 'lang\tbench\titers\tms\tchecksum\tus_per_op\n'
awk -F '\t' 'NF >= 5 { printf "%s\t%s\t%s\t%s\t%s\t%.6f\n", $1, $2, $3, $4, $5, ($4 * 1000.0) / $3 }' "$raw" \
  | sort -t$'\t' -k2,2 -k1,1
