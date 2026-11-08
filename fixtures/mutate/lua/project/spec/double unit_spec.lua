local calc = require("calc")

describe("calc double", function()
  it("doubles", function()
    assert.equal(4, calc.double(2))
  end)
end)
