local function place(rows, maxs, mins, column)
  if column == 8 then return true end
  for row = 0, 7 do
    if rows[row] and maxs[column + row] and mins[column - row + 7] then
      rows[row], maxs[column + row], mins[column - row + 7] = false, false, false
      if place(rows, maxs, mins, column + 1) then return true end
      rows[row], maxs[column + row], mins[column - row + 7] = true, true, true
    end
  end
  return false
end

return {
  warmup = 10,
  iters = 40,
  size = 1000,
  expected = 1000,
  run = function(size)
    local count = 0
    for _ = 1, size do
      local rows, maxs, mins = {}, {}, {}
      for i = 0, 7 do rows[i] = true end
      for i = 0, 14 do maxs[i], mins[i] = true, true end
      if place(rows, maxs, mins, 0) then count = count + 1 end
    end
    return count
  end,
}
