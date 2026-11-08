local calc = require("calc")

-- Test-only observability, as in add_spec.lua.
local marker = os.getenv("MUTATOR_LUA_SPEC_MARKER")
if marker then
  local file = io.open(marker, "a")
  if file then
    file:write("double " .. table.concat(arg or {}, " ") .. "\n")
    file:close()
  end
end

describe("calc double", function()
  it("doubles", function()
    assert.equal(4, calc.double(2))
  end)
end)
