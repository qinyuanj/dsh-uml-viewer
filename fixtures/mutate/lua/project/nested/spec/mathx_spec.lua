local mathx = require("mathx")

describe("mathx", function()
  it("halves", function()
    assert.equal(2, mathx.halve(4))
  end)
end)
