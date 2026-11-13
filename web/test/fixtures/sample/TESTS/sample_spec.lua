local sample = require('sample')

describe("sample", function()
  it("greets", function()
    assert.are.equal("hello you", sample.greet(" you "))
  end)
end)
