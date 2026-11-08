local calc = require("calc")

describe("calc add", function()
  it("adds one", function()
    assert.equal(2, calc.add(1))
  end)
end)
