local calc = require("calc")

-- Test-only observability: an absolute marker path outside the project, so a
-- host-side check can count which specs a mutant's command actually ran.
local marker = os.getenv("MUTATOR_LUA_SPEC_MARKER")
if marker then
  local file = io.open(marker, "a")
  if file then
    file:write("add " .. table.concat(arg or {}, " ") .. "\n")
    file:close()
  end
end

describe("calc add", function()
  it("adds one", function()
    assert.equal(2, calc.add(1))
  end)
end)
