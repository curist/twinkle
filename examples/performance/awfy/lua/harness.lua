local harness = {}

local function monotonic_ms()
  if type(unix) == "table" and type(unix.clock_gettime) == "function" then
    local seconds, nanoseconds = unix.clock_gettime(unix.CLOCK_MONOTONIC)
    return seconds * 1000 + nanoseconds / 1000000
  end
  return os.clock() * 1000
end

function harness.run(bench)
  local warm_sink = 0
  for _ = 1, bench.warmup do
    warm_sink = bench.compat.bxor(warm_sink, bench.run(bench.size))
  end
  if warm_sink == 0x7fffffff then
    print("unreachable", warm_sink)
  end

  local started = monotonic_ms()
  local checksum = 0
  for _ = 1, bench.iters do
    checksum = bench.run(bench.size)
  end
  local elapsed = monotonic_ms() - started

  if checksum ~= bench.expected then
    error(string.format("%s: checksum %s != expected %s", bench.name, checksum, bench.expected))
  end
  print(string.format("%s\t%s\t%d\t%.6f\t%d", bench.language, bench.name, bench.iters, elapsed, checksum))
end

return harness
