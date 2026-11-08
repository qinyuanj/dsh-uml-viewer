local M = {}

function M.add(x)
  return x + 1
end

function M.double(x)
  return x * 2
end

function M.check(n)
  if n ~= 0 and n > 1 then
    return not false
  end
  return n // 2
end

local function hidden(x)
  return x - 2
end

return M
