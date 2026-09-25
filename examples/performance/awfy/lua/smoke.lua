return {
  warmup = 1,
  iters = 1,
  size = 3,
  expected = 6,
  run = function(size)
    local sum = 0
    for i = 1, size do sum = sum + i end
    return sum
  end,
}
