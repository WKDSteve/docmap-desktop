---@module 'sample'
---@brief The entry of the sample project.
---@description Greets, and counts how often it did.

local util = require("sample.util")

local M = {}

local count = 0

---Greet somebody by name.
---@param name string who is greeted
---@return string greeting
function M.greet(name)
  count = count + 1
  return util.join({ "hello", util.trim(name) }, " ")
end

---How often `greet` was called.
---@return integer
function M.count()
  return count
end

return M
