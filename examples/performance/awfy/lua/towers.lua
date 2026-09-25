local function move_top(pegs, from, to)
  pegs[to][#pegs[to] + 1] = table.remove(pegs[from])
  return 1
end

local function move_disks(pegs, n, from, to, via)
  if n == 1 then return move_top(pegs, from, to) end
  local moves = move_disks(pegs, n - 1, from, via, to)
  moves = moves + move_top(pegs, from, to)
  return moves + move_disks(pegs, n - 1, via, to, from)
end

return {
  warmup = 10,
  iters = 20,
  size = 200,
  expected = 8191,
  run = function(size)
    local moves = 0
    for _ = 1, size do
      local first = {}
      for i = 1, 13 do first[i] = 14 - i end
      moves = move_disks({ first, {}, {} }, 13, 1, 3, 2)
    end
    return moves
  end,
}
