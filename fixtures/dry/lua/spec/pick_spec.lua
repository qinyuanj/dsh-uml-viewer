local pick = require("pick")

describe("pick", function()
  it("clamps", function()
    assert.equal(100, pick.clamp(200, 0, 100))
  end)
end)
