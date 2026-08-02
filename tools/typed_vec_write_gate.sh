#!/usr/bin/env bash
# Typed Vector<Int> indexed-write promotion (sub-project A) gate.
#
# Command-level checks that CANNOT live in the in-process boot suite:
#   - final-emit call targets (in-place vs functional set, typed reads, and the
#     ABSENCE of boxed vector ops on a promoted path) — emit-time decisions;
#   - the TWINKLE_TYPED_VEC_WRITE=0 kill-switch flip (one fixed process env per
#     `twk` invocation, so on/off cannot be mixed inside one suite run);
#   - an out-of-bounds indexed set that traps (an in-process trap would crash
#     the suite runner).
#
# Token matching is boundary-exact: `rt_arr__get` is a substring of
# `rt_arr__get_i64`, `set_in_place` of `set_in_place_i64`, `PVecI64` of `PVec`,
# so we extract whole `call $rt_arr__…` targets and compare them with `grep -qx`.
#
# Usage: tools/typed_vec_write_gate.sh   (expects target/twk already built)
set -euo pipefail

TWK=${TWK:-target/twk}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

# Whole call-target tokens emitted in a function's WAT (deduplicated).
call_targets() { grep -Eo 'call \$rt_arr__[A-Za-z0-9_]+' "$1" | sort -u; }
has_call()  { call_targets "$1" | grep -qx "call \$$2"; }
no_call()   { ! call_targets "$1" | grep -qx "call \$$2"; }
has_pvec()  { grep -Eq '\$rt_types__PVec([)[:space:]])' "$1"; }
no_pvec_i64() { ! grep -q '\$rt_types__PVecI64' "$1"; }

# Body of the user function `f`. Scoped assertions avoid runtime helper bodies
# turning an absence check into a module-level substring trap.
user_f_body() {
  wat=$1
  out=$2
  awk '
    /^  \(func \$user__\$f[0-9]+_f \(type / { in_func = 1 }
    in_func && /^  \(func / && seen { exit }
    in_func { print; seen = 1 }
  ' "$wat" > "$out"
  [ -s "$out" ] || fail "missing user f function body in $wat"
}

# The functype line of the user function `f`. Scoped to the promoted function —
# a raw module grep for PVecI64 is vacuous, since the PVecI64 type/global and
# PVecI64-returning runtime ops are emitted regardless of the flag.
user_f_type_line() {
  wat=$1
  type_name=$(grep -Eo '\(func \$user__\$f[0-9]+_f \(type \$functype_[0-9]+\)' "$wat" \
    | head -1 \
    | sed -E 's/.*\$(functype_[0-9]+).*/\1/')
  [ -n "$type_name" ] || fail "missing user f function type in $wat"
  awk -v t="$type_name" '$0 ~ "\\(type \\$" t " " { print; found = 1; exit } END { exit found ? 0 : 1 }' "$wat"
}

# ── Fixtures ────────────────────────────────────────────────────────
# Owned: locally built, indexed-set, read, returned → in-place + typed read.
cat > "$TMP/owned.tw" <<'EOF'
fn f(n: Int) Vector<Int> {
  xs := collect i in range(n) { i }
  xs[0] = 42
  y := xs[1]
  println(y.to_string())
  xs
}
println(f(5).len().to_string())
EOF

# Shared: an alias is read after the set, so ownership fails → functional set.
cat > "$TMP/shared.tw" <<'EOF'
fn f(n: Int) Int {
  xs := collect i in range(n) { i }
  ys := xs
  xs[0] = 42
  ys[0] + xs[0]
}
println(f(5).to_string())
EOF

# Vector.make producer (no loop), called twice so `f` is not inlined and its
# return type stays inspectable. Unlike a collect-born producer (whose return ABI
# is boxed by a pre-existing return_atom_slots limitation), a Vector.make producer
# gets a typed PVecI64 return, so this is the fixture for the return-type check.
cat > "$TMP/make.tw" <<'EOF'
fn f(n: Int) Vector<Int> {
  xs := Vector.make(n, 0)
  xs[1] = 9
  xs
}
println(f(3).len().to_string())
println(f(5).len().to_string())
EOF

# Set + persistent append: sub-project B extends the promoted indexed-write
# shape so the append also stays typed when the kill-switch is on.
cat > "$TMP/set_append.tw" <<'EOF'
fn f(n: Int) Int {
  xs := collect i in range(n) { i }
  xs[0] = 42
  xs = xs.append(99)
  xs[1] + xs[n]
}
println(f(5).to_string())
EOF

# Out-of-bounds indexed set on an owned vector → runtime trap.
cat > "$TMP/oob.tw" <<'EOF'
fn f(n: Int) Vector<Int> {
  xs := collect i in range(n) { i }
  xs[n + 5] = 1
  xs
}
println(f(4).len().to_string())
EOF

# ── 1. Owned, flag ON: typed in-place + typed read, no boxed vector ops ──
"$TWK" build "$TMP/owned.tw" -o "$TMP/owned_on.wat" >/dev/null
has_call "$TMP/owned_on.wat" "rt_arr__set_in_place_i64" || fail "owned flag-on: expected set_in_place_i64"
has_call "$TMP/owned_on.wat" "rt_arr__get_i64"          || fail "owned flag-on: expected get_i64"
no_call  "$TMP/owned_on.wat" "rt_arr__set_in_place"     || fail "owned flag-on: boxed set_in_place present"
no_call  "$TMP/owned_on.wat" "rt_arr__set"              || fail "owned flag-on: boxed set present"
no_call  "$TMP/owned_on.wat" "rt_arr__get"              || fail "owned flag-on: boxed get present"
echo "ok: owned flag-on emits set_in_place_i64 + get_i64, no boxed vector ops"

# ── 2. Vector.make, flag ON: typed PVecI64 return ABI ──
"$TWK" build "$TMP/make.tw" -o "$TMP/make_on.wat" >/dev/null
user_f_type_line "$TMP/make_on.wat" | grep -q '(result (ref null $rt_types__PVecI64))' \
  || fail "Vector.make flag-on: expected f to return PVecI64, got: $(user_f_type_line "$TMP/make_on.wat")"
echo "ok: Vector.make flag-on exposes PVecI64 return ABI"

# ── 3. Set+append, flag ON: typed in-place set + typed append/read ──
"$TWK" build "$TMP/set_append.tw" -o "$TMP/set_append_on.wat" >/dev/null
user_f_body "$TMP/set_append_on.wat" "$TMP/set_append_on_f.wat"
has_call "$TMP/set_append_on_f.wat" "rt_arr__set_in_place_i64" || fail "set+append flag-on: expected set_in_place_i64"
has_call "$TMP/set_append_on_f.wat" "rt_arr__push_i64"         || fail "set+append flag-on: expected push_i64"
has_call "$TMP/set_append_on_f.wat" "rt_arr__get_i64"          || fail "set+append flag-on: expected get_i64"
no_call  "$TMP/set_append_on_f.wat" "rt_arr__push"             || fail "set+append flag-on: boxed push present"
no_call  "$TMP/set_append_on_f.wat" "rt_arr__get"              || fail "set+append flag-on: boxed get present"
no_pvec_i64 "$TMP/set_append_on_f.wat" && fail "set+append flag-on: expected PVecI64 locals"
echo "ok: set+append flag-on emits push_i64 + set_in_place_i64 + get_i64, no boxed vector calls"

# ── 4. Owned, flag OFF: boxed path, no typed vector ops ──
TWINKLE_TYPED_VEC_WRITE=0 "$TWK" build "$TMP/owned.tw" -o "$TMP/owned_off.wat" >/dev/null
has_call "$TMP/owned_off.wat" "rt_arr__set_in_place"    || fail "owned flag-off: expected boxed set_in_place"
has_call "$TMP/owned_off.wat" "rt_arr__get"             || fail "owned flag-off: expected boxed get"
no_call  "$TMP/owned_off.wat" "rt_arr__set_i64"         || fail "owned flag-off: typed set_i64 present"
no_call  "$TMP/owned_off.wat" "rt_arr__set_in_place_i64" || fail "owned flag-off: typed set_in_place_i64 present"
no_call  "$TMP/owned_off.wat" "rt_arr__get_i64"         || fail "owned flag-off: typed get_i64 present"
echo "ok: owned flag-off stays boxed (kill-switch flips the whole path)"

# ── 5. Set+append, flag OFF: boxed set + boxed append/read ──
TWINKLE_TYPED_VEC_WRITE=0 "$TWK" build "$TMP/set_append.tw" -o "$TMP/set_append_off.wat" >/dev/null
user_f_body "$TMP/set_append_off.wat" "$TMP/set_append_off_f.wat"
has_call "$TMP/set_append_off_f.wat" "rt_arr__set_in_place" || fail "set+append flag-off: expected boxed set_in_place"
has_call "$TMP/set_append_off_f.wat" "rt_arr__push"         || fail "set+append flag-off: expected boxed push"
has_call "$TMP/set_append_off_f.wat" "rt_arr__get"          || fail "set+append flag-off: expected boxed get"
has_pvec "$TMP/set_append_off_f.wat"                        || fail "set+append flag-off: expected boxed PVec locals"
no_call  "$TMP/set_append_off_f.wat" "rt_arr__set_in_place_i64" || fail "set+append flag-off: typed set_in_place_i64 present"
no_call  "$TMP/set_append_off_f.wat" "rt_arr__push_i64"         || fail "set+append flag-off: typed push_i64 present"
no_call  "$TMP/set_append_off_f.wat" "rt_arr__get_i64"          || fail "set+append flag-off: typed get_i64 present"
no_pvec_i64 "$TMP/set_append_off_f.wat"                         || fail "set+append flag-off: typed PVecI64 present"
echo "ok: set+append flag-off stays boxed, including append"

# ── 6. Shared alias, flag ON: functional typed set, NOT in-place ──
"$TWK" build "$TMP/shared.tw" -o "$TMP/shared_on.wat" >/dev/null
has_call "$TMP/shared_on.wat" "rt_arr__set_i64"           || fail "shared flag-on: expected functional set_i64"
no_call  "$TMP/shared_on.wat" "rt_arr__set_in_place_i64"  || fail "shared flag-on: unexpected in-place on a shared vector"
echo "ok: shared/non-owned flag-on emits functional set_i64 (no in-place)"

# ── 7. Runtime: owned set reads back; alias unchanged; OOB traps ──
out=$("$TWK" run "$TMP/owned.tw" 2>&1); [ "$(printf '%s\n' "$out" | tail -1)" = "5" ] \
  || fail "owned run: expected len 5, got: $out"
out=$("$TWK" run "$TMP/shared.tw" 2>&1); [ "$(printf '%s\n' "$out" | tail -1)" = "42" ] \
  || fail "shared run: expected 42 (COW: alias 0 + new 42), got: $out"
if "$TWK" run "$TMP/oob.tw" >/dev/null 2>"$TMP/oob.err"; then
  fail "OOB set did not trap"
fi
grep -qi "out of bounds" "$TMP/oob.err" || fail "OOB set trapped without the expected message: $(cat "$TMP/oob.err")"
echo "ok: runtime — owned reads back, alias COW-preserved, OOB set traps"

echo "PASS: typed-vec write gate"
