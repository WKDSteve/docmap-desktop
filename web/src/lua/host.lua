-- The host of docmap.nvim's standalone engine inside the Lua VM of the web variant (wasmoon, Lua 5.4).
--
-- The sidecar of the desktop app is standalone/docmap.lua on PUC Lua 5.4 with two C modules (lfs, lua_tree_sitter) and
-- dkjson. Here the same pipeline (standalone/vim_shim.lua and lua/docmap/core, from the commit of engine.lock) runs on
-- the same Lua, and this file is what replaces the rest:
--
--   * `require` answers from the bundle the page carries (web/engine-bundle.mjs), never from a file;
--   * `lfs` is answered from the file system inside the VM (Emscripten's MEMFS, filled from the folder the person
--     picked), through the functions of `__web` (src/engine.js);
--   * `lua_tree_sitter`, the C parser, has no stand-in: the map has no facts per function (see the end of the file);
--   * `dkjson` is the small JSON module below: the shim only decodes with it (the engine writes its documents with
--     its own encoder, core/json.lua);
--   * there is no process to start: `io.popen` answers nil, so git and every other command are "not there";
--   * what the engine prints (io.stdout, io.stderr) is collected and handed back with the answer.
--
-- The doors `__web_generate`, `__web_api` and `__web_capabilities` are what standalone/docmap.lua does with its
-- arguments, with a request as JSON in place of argv. Every answer is one string: a status line, then the payload.

---@diagnostic disable: undefined-global, duplicate-set-field, lowercase-global
-- luacheck: std lua54
-- luacheck: globals print io os __web_generate __web_api __web_capabilities
-- luacheck: read globals vim
-- luacheck: read globals __web
-- luacheck: ignore 212/self

local web = __web

-- ------------------------------------------------------------------ require

local names = {}
for name in web.module_names():gmatch("[^\n]+") do
  names[name] = true
end

table.insert(package.searchers, 2, function(name)
  if not names[name] then
    return nil
  end
  local chunk, err = load(web.source(name), "@" .. name:gsub("%.", "/") .. ".lua")
  if not chunk then
    error(err, 0)
  end
  return chunk, name
end)

-- ---------------------------------------------------------------------- lfs

---`kind size mtime ino dev` of a path, or nil. `follow` false is lstat.
---@param path string
---@param follow boolean
---@return table?
local function stat(path, follow)
  local raw = web.stat(path, follow)
  if not raw then
    return nil
  end
  local kind, size, mtime, ino, dev = raw:match("^(%S+) (%d+) (%-?%d+) (%d+) (%d+)$")
  return {
    mode = kind,
    size = math.tointeger(size),
    modification = math.tointeger(mtime),
    access = math.tointeger(mtime),
    change = math.tointeger(mtime),
    ino = math.tointeger(ino),
    dev = math.tointeger(dev),
  }
end

---@param follow boolean
local function attributes(follow)
  return function(path, request)
    if type(path) ~= "string" then
      error("bad argument #1 to 'attributes' (string expected)", 2)
    end
    local st = stat(path, follow)
    if not st then
      return nil, ("cannot obtain information from file '%s': No such file or directory"):format(path), 2
    end
    if type(request) == "string" then
      if request == "target" then
        return web.readlink(path)
      end
      return st[request]
    end
    return st
  end
end

local lfs = {
  attributes = attributes(true),
  symlinkattributes = attributes(false),
}

function lfs.currentdir()
  return web.cwd()
end

function lfs.chdir(path)
  if web.chdir(path) then
    return true
  end
  return nil, ("Unable to change working directory to '%s'"):format(path)
end

function lfs.mkdir(path)
  if web.mkdir(path) then
    return true
  end
  return nil, "File exists or no such directory", 17
end

---The names in `path`, with `.` and `..` as LuaFileSystem gives them. Raises when the folder cannot be read.
function lfs.dir(path)
  local listed = web.readdir(path)
  if not listed then
    error(("cannot open %s: No such file or directory"):format(tostring(path)), 2)
  end
  local entries = { ".", ".." }
  -- One name per line would break on a name with a newline in it; the names come length-prefixed.
  local at = 1
  while at <= #listed do
    local len, rest = listed:match("^(%d+):()", at)
    local from = rest
    local to = from + math.tointeger(len) - 1
    entries[#entries + 1] = listed:sub(from, to)
    at = to + 1
  end
  local i = 0
  local dir_obj = {}
  function dir_obj:next()
    i = i + 1
    return entries[i]
  end
  function dir_obj:close()
    i = #entries
  end
  return function()
    return dir_obj:next()
  end, dir_obj
end

package.preload["lfs"] = function()
  return lfs
end

-- ------------------------------------------------------------------- dkjson

local json = {}
json.null = setmetatable({}, {
  __tostring = function()
    return "null"
  end,
})

local ESCAPES = { b = "\b", f = "\f", n = "\n", r = "\r", t = "\t", ["/"] = "/", ["\\"] = "\\", ['"'] = '"' }

---@param cp integer
---@return string
local function utf8_of(cp)
  return utf8.char(cp)
end

---@param s string
---@param pos integer
---@param nullval any
---@return any value, integer next
local function decode_value(s, pos, nullval, depth)
  if depth > 512 then
    error("nested too deep", 0)
  end
  pos = s:find("%S", pos) or (#s + 1)
  local c = s:sub(pos, pos)
  if c == "{" then
    local out = {}
    pos = s:find("%S", pos + 1) or (#s + 1)
    if s:sub(pos, pos) == "}" then
      return out, pos + 1
    end
    while true do
      local key
      pos = s:find("%S", pos) or (#s + 1)
      if s:sub(pos, pos) ~= '"' then
        error(("object key expected at %d"):format(pos), 0)
      end
      key, pos = decode_value(s, pos, nullval, depth + 1)
      pos = s:find("%S", pos) or (#s + 1)
      if s:sub(pos, pos) ~= ":" then
        error(("':' expected at %d"):format(pos), 0)
      end
      local value
      value, pos = decode_value(s, pos + 1, nullval, depth + 1)
      out[key] = value
      pos = s:find("%S", pos) or (#s + 1)
      local sep = s:sub(pos, pos)
      if sep == "}" then
        return out, pos + 1
      elseif sep ~= "," then
        error(("',' or '}' expected at %d"):format(pos), 0)
      end
      pos = pos + 1
    end
  elseif c == "[" then
    local out, n = {}, 0
    pos = s:find("%S", pos + 1) or (#s + 1)
    if s:sub(pos, pos) == "]" then
      return out, pos + 1
    end
    while true do
      local value
      value, pos = decode_value(s, pos, nullval, depth + 1)
      n = n + 1
      out[n] = value
      pos = s:find("%S", pos) or (#s + 1)
      local sep = s:sub(pos, pos)
      if sep == "]" then
        return out, pos + 1
      elseif sep ~= "," then
        error(("',' or ']' expected at %d"):format(pos), 0)
      end
      pos = pos + 1
    end
  elseif c == '"' then
    local parts, n = {}, 0
    local at = pos + 1
    while true do
      local stop = s:find('["\\]', at)
      if not stop then
        error(("unterminated string at %d"):format(pos), 0)
      end
      if stop > at then
        n = n + 1
        parts[n] = s:sub(at, stop - 1)
      end
      if s:sub(stop, stop) == '"' then
        return table.concat(parts), stop + 1
      end
      local e = s:sub(stop + 1, stop + 1)
      if e == "u" then
        local hex = s:match("^%x%x%x%x", stop + 2)
        if not hex then
          error(("bad \\u escape at %d"):format(stop), 0)
        end
        local cp = tonumber(hex, 16)
        at = stop + 6
        if cp >= 0xD800 and cp <= 0xDBFF then
          local low = s:match("^\\u(%x%x%x%x)", at)
          local lcp = low and tonumber(low, 16)
          if lcp and lcp >= 0xDC00 and lcp <= 0xDFFF then
            cp = 0x10000 + (cp - 0xD800) * 0x400 + (lcp - 0xDC00)
            at = at + 6
          end
        end
        n = n + 1
        parts[n] = utf8_of(cp)
      else
        local plain = ESCAPES[e]
        if not plain then
          error(("bad escape at %d"):format(stop), 0)
        end
        n = n + 1
        parts[n] = plain
        at = stop + 2
      end
    end
  elseif s:sub(pos, pos + 3) == "true" then
    return true, pos + 4
  elseif s:sub(pos, pos + 4) == "false" then
    return false, pos + 5
  elseif s:sub(pos, pos + 3) == "null" then
    return nullval, pos + 4
  end
  local num = s:match("^-?%d+%.?%d*[eE]?[+-]?%d*", pos)
  local value = num and num ~= "" and tonumber(num)
  if not value then
    error(("no valid JSON value at %d"):format(pos), 0)
  end
  return value, pos + #num
end

---dkjson's `decode(text, position, null)`: the value and the position after it, or nil, the position and a message.
function json.decode(s, pos, nullval)
  local ok, value, next_pos = pcall(decode_value, s, pos or 1, nullval, 0)
  if not ok then
    return nil, pos or 1, tostring(value)
  end
  return value, next_pos
end

local function encode_value(value, out)
  local ty = type(value)
  if value == nil or value == json.null then
    out[#out + 1] = "null"
  elseif ty == "boolean" then
    out[#out + 1] = value and "true" or "false"
  elseif ty == "number" then
    out[#out + 1] = math.type(value) == "integer" and tostring(value) or ("%.14g"):format(value)
  elseif ty == "string" then
    out[#out + 1] = '"'
      .. value:gsub('[%c"\\]', function(ch)
        local known = {
          ['"'] = '\\"',
          ["\\"] = "\\\\",
          ["\b"] = "\\b",
          ["\f"] = "\\f",
          ["\n"] = "\\n",
          ["\r"] = "\\r",
          ["\t"] = "\\t",
        }
        return known[ch] or ("\\u%04x"):format(ch:byte())
      end)
      .. '"'
  elseif ty == "table" then
    local n = #value
    if n > 0 or next(value) == nil then
      out[#out + 1] = "["
      for i = 1, n do
        if i > 1 then
          out[#out + 1] = ","
        end
        encode_value(value[i], out)
      end
      out[#out + 1] = "]"
    else
      local keys = {}
      for k in pairs(value) do
        keys[#keys + 1] = tostring(k)
      end
      table.sort(keys)
      out[#out + 1] = "{"
      for i, k in ipairs(keys) do
        if i > 1 then
          out[#out + 1] = ","
        end
        encode_value(k, out)
        out[#out + 1] = ":"
        encode_value(value[k], out)
      end
      out[#out + 1] = "}"
    end
  else
    error("cannot encode a " .. ty, 0)
  end
end

function json.encode(value)
  local out = {}
  encode_value(value, out)
  return table.concat(out)
end

package.preload["dkjson"] = function()
  return json
end

-- ------------------------------------------------- processes, clock, output

---No process can be started in a browser. nil is what `io.popen` gives where it cannot start one, and what the shim
---and the engine already treat as "the command is not there".
io.popen = function()
  return nil, "no process can be started in the browser"
end

os.getenv = function(name)
  return web.env(name)
end

os.exit = function(code)
  error({ exit = code }, 0)
end

local captured = {}

---Keep one piece of output: the stream (`o` or `e`), the length of the text in bytes, a colon, the text. No separator
---byte between the pieces: a NUL in a Lua string ends the string on its way to the page.
---@param stream string
---@param text string
local function capture(stream, text)
  captured[#captured + 1] = ("%s%d:"):format(stream, #text) .. text
end

local function sink(stream)
  local file = {}
  function file:write(...)
    for i = 1, select("#", ...) do
      capture(stream, tostring((select(i, ...))))
    end
    return self
  end
  function file:flush()
    return self
  end
  function file:close()
    return true
  end
  function file:setvbuf()
    return true
  end
  return file
end
io.stdout = sink("o")
io.stderr = sink("e")
print = function(...)
  local parts = {}
  for i = 1, select("#", ...) do
    parts[i] = tostring((select(i, ...)))
  end
  capture("o", table.concat(parts, "\t") .. "\n")
end

-- ------------------------------------------------------------- the pipeline

require("standalone.vim_shim")

-- No parser: `vim.treesitter` stays the inert stub of the shim, and the engine writes the map without the facts per
-- function (functions, calls), exactly as the sidecar does where it finds no grammar.

local encode = function(value)
  return require("docmap.core.json").encode(value)
end

---The request of a door: JSON, decoded with nulls dropped.
---@param text string
---@return table
local function request_of(text)
  if text == nil or text == "" then
    return {}
  end
  return vim.json.decode(text, { luanil = { object = true, array = true } })
end

---Run `fn`, with what it printed. An `os.exit(code)` inside ends it with that code.
---@return string status, string payload, string log
local function guarded(fn)
  captured = {}
  local ok, result = xpcall(fn, function(err)
    if type(err) == "table" and err.exit ~= nil then
      return err
    end
    return debug.traceback(tostring(err), 2)
  end)
  local log = table.concat(captured)
  captured = {}
  if ok then
    return "ok", result, log
  end
  if type(result) == "table" then
    return "exit", tostring(result.exit), log
  end
  return "error", tostring(result), log
end

---One answer string: the status, the length of the payload, the payload, then the log (the pieces as `capture` wrote
---them).
local function answer(status, payload, log)
  payload = payload or ""
  return ("%s %d\n"):format(status, #payload) .. payload .. (log or "")
end

---The options of a run, as standalone/docmap.lua builds them from its arguments.
---@param req table
local function options(req)
  local notify = {
    warn = function(msg)
      io.stderr:write("docmap: " .. tostring(msg) .. "\n")
    end,
  }
  local source = req.source
  if type(source) == "table" and #source == 1 then
    source = source[1]
  elseif type(source) == "table" and #source == 0 then
    source = nil
  end
  return require("docmap.config").build(req.root, {
    source = source,
    exclude = (type(req.exclude) == "table" and #req.exclude > 0) and req.exclude or nil,
    languages = (type(req.languages) == "table" and #req.languages > 0) and req.languages or nil,
    repo_url = req.repo_url,
    branch = req.branch,
    out_dir = req.out_dir,
  }, notify)
end

---`docmap <root> [flags]`: the map of `root`, written into its out_dir inside the VM. The payload is the exit code.
function __web_generate(request)
  return answer(guarded(function()
    local req = request_of(request)
    local code = require("docmap.core.cli").run(options(req), req.argv or {})
    return tostring(code)
  end))
end

---`docmap <root> --api=<route> [--snapshot=<param>]`: one route of core/api.lua. There is no git here: the routes
---that need it say so.
function __web_api(request)
  return answer(guarded(function()
    local req = request_of(request)
    local result, err = require("docmap.core.api").answer(req.route, options(req), req.snapshot)
    if not result then
      io.stderr:write(tostring(err) .. "\n")
      error({ exit = 2 }, 0)
    end
    return encode(result)
  end))
end

---`docmap --capabilities`.
function __web_capabilities()
  return answer(guarded(function()
    local routes = {}
    for name in pairs(require("docmap.core.api").routes) do
      routes[#routes + 1] = name
    end
    table.sort(routes)
    return encode({
      capabilities = { "api", "languages", "schema" },
      schema = require("docmap.core.scan").SCHEMA,
      routes = routes,
      languages = require("docmap.core.lang_registry").report(),
    })
  end))
end
