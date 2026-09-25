local input = '{"widget":{"debug":"on","window":{"title":"Sample","width":500,"height":300},"items":[1,2,3,4,5],"enabled":true,"data":null}}'

local function parse(source)
  local index = 1
  local value
  local function skip_whitespace()
    while source:sub(index, index):match("%s") do index = index + 1 end
  end
  local function parse_string()
    index = index + 1
    local start = index
    while source:sub(index, index) ~= '"' do index = index + 1 end
    local result = source:sub(start, index - 1)
    index = index + 1
    return result
  end
  local function parse_number()
    local start = index
    if source:sub(index, index) == "-" then index = index + 1 end
    while source:sub(index, index):match("%d") do index = index + 1 end
    return tonumber(source:sub(start, index - 1))
  end
  local function parse_array()
    index = index + 1
    local items = {}
    skip_whitespace()
    if source:sub(index, index) == "]" then index = index + 1; return { kind = "array", value = items } end
    while true do
      items[#items + 1] = value()
      skip_whitespace()
      if source:sub(index, index) == "," then index = index + 1 else index = index + 1; break end
    end
    return { kind = "array", value = items }
  end
  local function parse_object()
    index = index + 1
    local entries = {}
    skip_whitespace()
    if source:sub(index, index) == "}" then index = index + 1; return { kind = "object", value = entries } end
    while true do
      skip_whitespace()
      local key = parse_string()
      skip_whitespace()
      index = index + 1
      entries[#entries + 1] = { key = key, value = value() }
      skip_whitespace()
      if source:sub(index, index) == "," then index = index + 1 else index = index + 1; break end
    end
    return { kind = "object", value = entries }
  end
  value = function()
    skip_whitespace()
    local char = source:sub(index, index)
    if char == "{" then return parse_object() end
    if char == "[" then return parse_array() end
    if char == '"' then return { kind = "string", value = parse_string() } end
    if char == "t" then index = index + 4; return { kind = "boolean", value = true } end
    if char == "f" then index = index + 5; return { kind = "boolean", value = false } end
    if char == "n" then index = index + 4; return { kind = "null" } end
    return { kind = "number", value = parse_number() }
  end
  return value()
end

local function fold(node, acc)
  acc.count = acc.count + 1
  if node.kind == "number" then
    acc.sum = acc.sum + node.value
  elseif node.kind == "array" then
    for _, child in ipairs(node.value) do fold(child, acc) end
  elseif node.kind == "object" then
    for _, entry in ipairs(node.value) do fold(entry.value, acc) end
  end
end

return {
  warmup = 20,
  iters = 100,
  size = 1000,
  expected = 25280,
  run = function(size)
    local checksum = 0
    for _ = 1, size do
      local acc = { sum = 0, count = 0 }
      fold(parse(input), acc)
      checksum = acc.sum * 31 + acc.count
    end
    return checksum
  end,
}
