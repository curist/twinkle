local compat = ...

local function make_rng()
  local seed = 74755
  return function()
    seed = compat.band(seed * 1309 + 13849, 65535)
    return seed
  end
end

return {
  warmup = 10,
  iters = 20,
  size = 120,
  expected = 27881,
  run = function(size)
    local result = 0
    for _ = 1, size do
      local next_value, count, leaf_elems = make_rng(), 0, 0
      local function build(depth)
        count = count + 1
        if depth == 1 then
          local length = (next_value() % 10) + 1
          leaf_elems = leaf_elems + length
          local leaf = {}
          for i = 1, length do leaf[i] = false end
          return leaf
        end
        local children = {}
        for i = 1, 4 do children[i] = build(depth - 1) end
        return children
      end
      build(7)
      result = count + leaf_elems
    end
    return result
  end,
}
