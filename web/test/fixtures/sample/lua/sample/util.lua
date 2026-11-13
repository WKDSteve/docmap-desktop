---@module 'sample.util'
---@brief Small string helpers: Grüße need them too.

local M = {}

---Cut the white space at both ends.
---@param s string
---@return string
function M.trim(s)
  return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end

---Join the parts with a separator.
---@param parts string[]
---@param sep string
---@return string
function M.join(parts, sep)
  if #parts == 0 then
    return ""
  end
  return table.concat(parts, sep)
end

return M
