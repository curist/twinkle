#!/usr/bin/env python3
"""Summarize vector/container specialization opportunities in generated WAT.

Usage:
  target/twk ir boot/main.tw --wat > /tmp/boot.wat
  python3 tools/analyze_vector_wat.py /tmp/boot.wat

The output is intentionally static: it counts emitted call/cast/allocation sites,
not dynamic execution frequency. Use it to decide where to add more focused
runtime timings or specialized helper families.
"""

from __future__ import annotations

import argparse
import re
from collections import Counter, defaultdict
from pathlib import Path


def decode_user_func(sym: str) -> str:
    m = re.match(r"user__\$f(\d+)_(\d+)$", sym)
    if not m:
        return sym
    digits = m.group(2)
    out: list[str] = []
    i = 0
    while i < len(digits):
        if i + 3 <= len(digits) and 100 <= int(digits[i : i + 3]) <= 122:
            out.append(chr(int(digits[i : i + 3])))
            i += 3
        elif i + 2 <= len(digits):
            out.append(chr(int(digits[i : i + 2])))
            i += 2
        else:
            break
    return f"{m.group(1)}:{''.join(out)}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("wat", type=Path)
    parser.add_argument("--top", type=int, default=12)
    args = parser.parse_args()

    lines = args.wat.read_text(errors="ignore").splitlines()
    totals = Counter()
    per_func: dict[str, Counter[str]] = defaultdict(Counter)
    func = "<module>"

    for i, line in enumerate(lines):
        m = re.match(r"  \(func \$([^ ]+)", line)
        if m:
            func = m.group(1)

        events: list[str] = []
        if "call $rt_arr__push" in line:
            events.append("push")
        if "call $rt_arr__get" in line:
            events.append("get")
        if "call $rt_arr__set" in line:
            events.append("set")
        if "call $rt_arr__builder_push" in line:
            events.append("builder_push")
        if "call $rt_arr__builder_freeze" in line:
            events.append("builder_freeze")
        if "call $rt_arr__builder_from" in line:
            events.append("builder_from")
        if "struct.new $rt_types__BoxedInt" in line:
            events.append("box_int")
        if "struct.new $rt_types__BoxedFloat" in line:
            events.append("box_float")
        if "struct.get $rt_types__BoxedInt" in line:
            events.append("unbox_int")
        if "struct.get $rt_types__BoxedFloat" in line:
            events.append("unbox_float")
        if "array.get $rt_types__Array" in line:
            events.append("array_get_anyref")
        if "array.set $rt_types__Array" in line:
            events.append("array_set_anyref")

        if "call $rt_arr__get" in line:
            following = "\n".join(lines[i + 1 : i + 7])
            if "ref.cast (ref $rt_types__BoxedInt)" in following:
                events.append("get_to_int")
            if "ref.cast (ref $" in following:
                events.append("get_to_named_ref")
            if "ref.cast (ref i31)" in following:
                events.append("get_to_i31")

        if "call $rt_arr__push" in line or "call $rt_arr__builder_push" in line:
            previous = "\n".join(lines[max(0, i - 6) : i])
            if "struct.new $rt_types__BoxedInt" in previous:
                events.append("push_from_boxed_int")
            if "struct.new $rt_types__BoxedFloat" in previous:
                events.append("push_from_boxed_float")

        for event in events:
            totals[event] += 1
            per_func[func][event] += 1

    print("Static vector/container WAT profile")
    print("===================================")
    for name, count in totals.most_common():
        print(f"{name:22} {count}")

    for event in ["get", "push", "builder_push", "box_int", "get_to_named_ref", "get_to_int"]:
        print(f"\nTop functions by {event}:")
        rows = sorted(per_func.items(), key=lambda kv: kv[1][event], reverse=True)
        for sym, counts in rows[: args.top]:
            if counts[event] == 0:
                break
            summary = " ".join(f"{k}={v}" for k, v in counts.most_common() if v)
            print(f"  {decode_user_func(sym):46} {summary}")


if __name__ == "__main__":
    main()
