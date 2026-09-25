return {
  warmup = 10,
  iters = 40,
  size = 5000,
  expected = 669,
  run = function(size)
    local flags = {}
    for i = 0, size do flags[i] = true end
    local count = 0
    for i = 2, size do
      if flags[i] then
        count = count + 1
        for k = i + i, size, i do flags[k] = false end
      end
    end
    return count
  end,
}
