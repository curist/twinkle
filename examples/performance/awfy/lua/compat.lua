local compat = {}

if type(bit) == "table" then
  compat.band = bit.band
  compat.bxor = bit.bxor
  compat.lshift = bit.lshift
else
  compat.band = assert(load("return function(a, b) return a & b end"))()
  compat.bxor = assert(load("return function(a, b) return a ~ b end"))()
  compat.lshift = assert(load("return function(a, b) return a << b end"))()
end

return compat
