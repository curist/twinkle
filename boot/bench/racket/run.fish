#!/usr/bin/env fish
# Side-by-side benchmark: Twinkle Vector vs Racket treelist (Rhombus's list type).
#
#   boot/bench/racket/run.fish
#
# For each workload it runs both the Twinkle .tw (via target/twk) and the Racket
# .rkt mirror, doing ONE discarded warmup pass + 5 timed passes and keeping the
# MIN ms per N on each side (both V8 and Racket CS need warmup, so this is the
# fair protocol). Prints: N | twk ms | racket ms | racket/twk.
#
# CAVEAT: this compares two whole stacks (Twinkle->wasm->Deno/V8 vs treelist on
# Racket CS), not data structures in isolation. Trust the SCALING (ms per
# doubling of N) and the relative shape, not the absolute ratio.

set -l repo (cd (dirname (status filename))/../../..; and pwd)
cd $repo

set -l twk $repo/target/twk
if not test -x $twk
    echo "error: $twk not found — run `make bundle-cli` first" >&2
    exit 1
end

# run a command k+1 times (1 warmup discarded), print "N<TAB>min_ms" per N.
function bench_min --argument-names cmd
    set -l tmp (mktemp)
    eval $cmd >/dev/null 2>&1            # warmup
    for i in (seq 5)
        eval $cmd 2>/dev/null
    end >$tmp
    awk -F'\t' '$1 ~ /^[0-9]+$/ { if (!($1 in m) || $2 < m[$1]) m[$1]=$2 }
                END { for (k in m) print k"\t"m[k] }' $tmp | sort -n
    rm -f $tmp
end

set -l workloads concat_prepend concat_append append_push get_regular get_relaxed slice_dropfirst

for name in $workloads
    echo ""
    echo "### $name"
    printf "%-8s %12s %12s %12s\n" N twk_ms racket_ms racket/twk
    set -l twk_out (mktemp)
    set -l rkt_out (mktemp)
    bench_min "$twk run boot/bench/$name.tw" >$twk_out
    bench_min "racket boot/bench/racket/$name.rkt" >$rkt_out
    # merge on N: twk first into t[], then walk racket rows (already sorted by N).
    awk -F'\t' '
        NR==FNR { t[$1]=$2; next }
        { tv=t[$1]; r=$2;
          printf "%-8s %12.3f %12.3f %11.2fx\n", $1, tv, r, (tv>0 ? r/tv : 0) }
    ' $twk_out $rkt_out
    rm -f $twk_out $rkt_out
end
