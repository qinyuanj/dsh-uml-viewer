local M = {}

local function between(x, low, high)
  return x >= low and x <= high
end

function M.clamp(x, low, high)
  if between(x, low, high) then
    return x
  end
  return x < low and low or high
end

M.untested = function(flag)
  if flag then
    return 1
  end
  return 0
end

M.spec = function(xs)
  local total = 0
  for _, x in ipairs(xs) do
    total = total + #tostring(x)
  end
  return total
end

return function(x)
  return x
end
