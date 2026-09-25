local function build(size)
  local result = nil
  for i = 0, size - 1 do result = { value = i, next = result } end
  return result
end

local function sum_list(list)
  local sum = 0
  while list do
    sum = sum + list.value
    list = list.next
  end
  return sum
end

return {
  warmup = 10,
  iters = 40,
  size = 1000,
  expected = 499500,
  run = function(size) return sum_list(build(size)) end,
}
