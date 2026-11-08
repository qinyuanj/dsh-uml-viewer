local M = {}

function M.choose(x, ready)
  if x > 0 and x < 10 then
    return 1
  elseif x == 0 or ready then
    return 0
  end
  for i = 1, x do
    if i == 2 then return i end
  end
  for _, v in ipairs(ready) do
    while v do break end
  end
  repeat x = x - 1 until x <= 0
  goto done
  ::done::
  return x
end

function Account:deposit(amount)
  self.balance = self.balance + amount
end

local function helper(n)
  return n and n > 0 or false
end

local first, second = 1, function(s)
  if s then return s end
end

M.bar = function() return 1 end

function global_fn() end

local function outer(xs)
  local function inner(n)
    if n then return 1 end
    return 0
  end
  local pick = function(n) return n or 0 end
  table.sort(xs, function(a, b) return a < b and true end)
  return inner(1)
end

table.sort({}, function(a, b) if a then return true end end)
local t = { f = function() end }

function literal()
  local message = "if x and y then"
  -- if x or y then
  --[[ while true do end ]]
  return message
end

local local_start =
  function()
    return 1
  end
return M
