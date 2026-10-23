--!nonstrict
-- Shared helpers for the magpie entries: the command, running it, the
-- published state, and small formatting bits.
--
-- State published by the `watch` service on STATE_KEY:
--
--   { status = "connecting" | "ready" | "error", error = string?,
--     shots = { ShotView }, updated = ms }
--
-- where ShotView is one element of `magpie list --json`:
--
--   { id, taken_at, original, original_exists, thumb?,
--     variants = { { id, parent, created, png, thumb? } } }
--
-- Everything goes through argv runAsync, so nothing passes through a shell.

local M = {}

M.STATE_KEY = "magpie"
M.SEEN_KEY = "magpie.seen" -- newest shot id the panel has shown
M.REFRESH_KEY = "magpie.refresh" -- bumped by the panel to request a re-list
M.COMMAND_KEY = "magpie.command" -- IPC override for the executable
M.PANEL = "toms/magpie:shots"
M.ORIGINAL = "original"

local ACTION_TIMEOUT_MS = 15000

function M.trim(text)
  if type(text) ~= "string" then
    return ""
  end
  return (string.gsub(text, "^%s*(.-)%s*$", "%1"))
end

function M.shellQuote(text)
  return "'" .. string.gsub(text, "'", "'\\''") .. "'"
end

-- Where `cargo install` and friends put binaries, which the shell's PATH often
-- lacks. Tried in order when a bare command name is not on PATH.
local FALLBACK_DIRS = { "/.cargo/bin/", "/.local/bin/" }

local function resolve(command)
  if string.find(command, "/", 1, true) or noctalia.commandExists(command) then
    return command
  end
  local home = noctalia.getenv("HOME")
  if home then
    for _, dir in FALLBACK_DIRS do
      local path = home .. dir .. command
      if noctalia.fileExists(path) then
        return path
      end
    end
  end
  return command
end

-- The magpie executable: the IPC override, else the plugin setting, else
-- "magpie". "~/" is expanded by hand; noctalia.expandPath would turn a bare
-- name into a plugin-relative path.
function M.command()
  local command = noctalia.state.get(M.COMMAND_KEY)
  if type(command) ~= "string" or M.trim(command) == "" then
    command = noctalia.getConfig("magpie_command")
  end
  command = M.trim(command)
  if command == "" then
    command = "magpie"
  end
  if string.sub(command, 1, 2) == "~/" then
    command = (noctalia.getenv("HOME") or "~") .. string.sub(command, 2)
  end
  return resolve(command)
end

local function argv(args)
  local list = { M.command() }
  for _, arg in ipairs(args) do
    list[#list + 1] = arg
  end
  return list
end

local function failure(result)
  local reason = M.trim(result.stderr)
  if reason == "" then
    reason = M.trim(result.stdout)
  end
  reason = string.gsub(reason, "^Error:%s*", "")
  if reason == "" then
    reason = "exit " .. tostring(result.exitCode)
  end
  return reason
end

-- Runs `magpie <args...>`. A failure raises an error notification; onDone,
-- when given, gets true or false.
function M.run(args, onDone)
  local started = noctalia.runAsync(argv(args), function(result)
    local ok = result.exitCode == 0 and not result.timedOut
    if not ok then
      noctalia.notifyError(noctalia.tr("notify.action_failed", { action = args[1] }), failure(result))
    end
    if onDone then
      onDone(ok)
    end
  end, ACTION_TIMEOUT_MS)
  if not started then
    noctalia.notifyError(noctalia.tr("notify.action_failed", { action = args[1] }), M.command())
    if onDone then
      onDone(false)
    end
  end
  return started
end

-- Starts `magpie <args...>` detached (the editor outlives any timeout).
function M.spawn(args)
  return noctalia.runAsync(argv(args))
end

-- `magpie list --json --limit n`. onDone(shots, nil) or onDone(nil, reason).
function M.list(limit, onDone)
  local started = noctalia.runAsync(argv({ "list", "--json", "--limit", tostring(limit) }), function(result)
    local out = M.trim(result.stdout)
    if result.exitCode == 0 and string.sub(out, 1, 1) == "[" then
      local ok, value = pcall(noctalia.json.decode, out)
      if ok and type(value) == "table" then
        onDone(value, nil)
        return
      end
    end
    if result.exitCode == 127 then
      onDone(nil, noctalia.tr("status.command_missing", { command = M.command() }))
    else
      onDone(nil, failure(result))
    end
  end, ACTION_TIMEOUT_MS)
  if not started then
    onDone(nil, noctalia.tr("status.command_missing", { command = M.command() }))
  end
end

-- ── Published state ──────────────────────────────────────────────────────────

function M.read()
  local value = noctalia.state.get(M.STATE_KEY)
  if type(value) == "table" then
    return value
  end
  return { status = "connecting", shots = {} }
end

function M.shots(value)
  return (value and type(value.shots) == "table") and value.shots or {}
end

-- Shots newer than the last one the panel showed.
function M.newCount(value)
  local seen = noctalia.state.get(M.SEEN_KEY)
  if type(seen) ~= "string" then
    return 0
  end
  local count = 0
  for _, shot in ipairs(M.shots(value)) do
    if shot.id > seen then
      count += 1
    end
  end
  return count
end

-- Persists the seen marker so the count survives a shell restart.
local function seenFile()
  local dir = noctalia.pluginDataDir()
  return dir and (dir .. "/seen") or nil
end

function M.loadSeen()
  local path = seenFile()
  if not path then
    return nil
  end
  local contents = noctalia.readFile(path)
  local id = M.trim(contents)
  return id ~= "" and id or nil
end

function M.markSeen(id)
  if type(id) ~= "string" or id == noctalia.state.get(M.SEEN_KEY) then
    return
  end
  noctalia.state.set(M.SEEN_KEY, id)
  local path = seenFile()
  if path then
    noctalia.writeFile(path, id .. "\n")
  end
end

function M.requestRefresh()
  noctalia.state.set(M.REFRESH_KEY, noctalia.nowMs())
end

-- ── Formatting ───────────────────────────────────────────────────────────────

-- "Today 22:51", "Yesterday 09:03" or "21 Oct 14:00" from an RFC 3339 time.
function M.when(stamp)
  if type(stamp) ~= "string" then
    return ""
  end
  local y, mo, d, h, mi = string.match(stamp, "^(%d+)-(%d+)-(%d+)T(%d+):(%d+)")
  if not y then
    return stamp
  end
  local day = y .. "-" .. mo .. "-" .. d
  local clock = h .. ":" .. mi
  if day == os.date("%Y-%m-%d") then
    return noctalia.tr("panel.today") .. " " .. clock
  elseif day == os.date("%Y-%m-%d", os.time() - 86400) then
    return noctalia.tr("panel.yesterday") .. " " .. clock
  end
  local months = { "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec" }
  return tostring(tonumber(d)) .. " " .. (months[tonumber(mo)] or mo) .. " " .. clock
end

-- `<id>` for the original, `<id>@<version>` for a variant.
function M.target(shotId, version)
  if version == nil or version == M.ORIGINAL then
    return shotId
  end
  return shotId .. "@" .. version
end

-- A shot's variants in tree order (each after its parent), with depth: 1 for
-- variants of the original, 2 for variants of those, and so on.
function M.variantTree(shot)
  local children = {}
  for _, variant in ipairs(shot.variants or {}) do
    local parent = variant.parent or M.ORIGINAL
    children[parent] = children[parent] or {}
    table.insert(children[parent], variant)
  end
  local out = {}
  local visited = {}
  local function walk(parent, depth)
    for _, variant in ipairs(children[parent] or {}) do
      if not visited[variant.id] then
        visited[variant.id] = true
        out[#out + 1] = { variant = variant, depth = depth }
        walk(variant.id, depth + 1)
      end
    end
  end
  walk(M.ORIGINAL, 1)
  -- Variants whose parent is gone still get listed, at the top level.
  for _, variant in ipairs(shot.variants or {}) do
    if not visited[variant.id] then
      visited[variant.id] = true
      out[#out + 1] = { variant = variant, depth = 1 }
      walk(variant.id, 2)
    end
  end
  return out
end

-- Renders `build()` into the panel, or the error on the surface when it throws.
function M.renderPanel(build)
  local ok, treeOrError = pcall(build)
  if ok then
    panel.render(treeOrError)
  else
    panel.render(ui.column({ flexGrow = 1, gap = 8, padding = 8 }, {
      ui.label({ text = noctalia.tr("panel.render_error"), fontWeight = "bold", color = "error" }),
      ui.label({ text = tostring(treeOrError), fontSize = 12 }),
    }))
  end
end

return M
