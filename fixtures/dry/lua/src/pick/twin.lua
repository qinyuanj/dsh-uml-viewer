local M = {}

local function bounded(value, low, high)
  return value >= low and value <= high
end

function M.limit(value, low, high)
  if bounded(value, low, high) then
    return value
  end
  return value < low and low or high
end

M.checked = function(flag)
  if flag then
    return 1
  end
  return 0
end

M.counted = function(items)
  local total = 0
  for _, item in ipairs(items) do
    total = total + #tostring(item)
  end
  return total
end

return M
