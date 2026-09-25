local source = arg[0]
local base = source:match("^(.*[/\\])") or ""
local harness = assert(loadfile(base .. "harness.lua"))()
local compat = assert(loadfile(base .. "compat.lua"))()
local names = { "smoke", "mandelbrot", "sieve", "queens", "permute", "towers", "list", "bounce", "storage", "nbody", "json" }
local selected = os.getenv("AWFY_BENCH")
local language = os.getenv("AWFY_LANG") or "lua-redbean"

for _, name in ipairs(names) do
  if not selected or selected == name then
    local bench = assert(loadfile(base .. name .. ".lua"))(compat)
    bench.name = name
    bench.language = language
    bench.compat = compat
    harness.run(bench)
  end
end
