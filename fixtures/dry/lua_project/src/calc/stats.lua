local util = require("calc.util")

local M = {}

function M.sum_clamped(values)
  local acc = 0
  for _, value in ipairs(values) do
    acc = acc + util.clamp(value, 0, 100)
  end
  return acc
end

return M
