local M = {}

function M.is_even(n)
  return n % 2 == 0
end

function M.halve(n)
  return n // 2
end

return M
