local compat = ...

return {
  warmup = 10,
  iters = 20,
  size = 500,
  expected = 191,
  run = function(size)
    local sum, byte_acc, bit_num = 0, 0, 0
    for y = 0, size - 1 do
      local ci = (2.0 * y / size) - 1.0
      for x = 0, size - 1 do
        local zr, zrzr, zi, zizi = 0.0, 0.0, 0.0, 0.0
        local cr = (2.0 * x / size) - 1.5
        local z, not_done, escape = 0, true, 0
        while not_done and z < 50 do
          zr = zrzr - zizi + cr
          zi = 2.0 * zr * zi + ci
          zrzr = zr * zr
          zizi = zi * zi
          if zrzr + zizi > 4.0 then not_done, escape = false, 1 end
          z = z + 1
        end
        byte_acc = (byte_acc << 1) + escape
        bit_num = bit_num + 1
        if bit_num == 8 then
          sum, byte_acc, bit_num = compat.bxor(sum, byte_acc), 0, 0
        elseif x == size - 1 then
          byte_acc = compat.lshift(byte_acc, 8 - bit_num)
          sum, byte_acc, bit_num = compat.bxor(sum, byte_acc), 0, 0
        end
      end
    end
    return sum
  end,
}
