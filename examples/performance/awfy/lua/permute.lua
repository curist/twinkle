local values
local count

local function swap(i, j)
  values[i], values[j] = values[j], values[i]
end

local function permute(n)
  count = count + 1
  if n ~= 0 then
    permute(n - 1)
    for i = n - 1, 0, -1 do
      swap(n - 1, i)
      permute(n - 1)
      swap(n - 1, i)
    end
  end
end

return {
  warmup = 10,
  iters = 20,
  size = 300,
  expected = 8660,
  run = function(size)
    for _ = 1, size do
      count, values = 0, {}
      for i = 0, 5 do values[i] = 0 end
      permute(6)
    end
    return count
  end,
}
