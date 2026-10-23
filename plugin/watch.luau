--!nonstrict
-- Service entry `watch`: the only entry that keeps a process open.
--
-- Runs `magpie watch` through runStream. That process imports every niri
-- screenshot and prints one JSON line per library change; each line triggers
-- a `magpie list --json`, published on the "magpie" state key (see
-- lib/magpie.luau). When the stream ends it retries with backoff (2s .. 30s).
--
-- runStream never reports its child exiting, so the stream is wrapped in a
-- tiny shell script that prints the watch pid first (so a restart can kill
-- it) and a sentinel line with the exit code last.
--
--   noctalia msg plugin toms/magpie:watch all refresh
--   noctalia msg plugin toms/magpie:watch all reconnect
--   noctalia msg plugin toms/magpie:watch all command /path/to/magpie   ("" clears)

local magpie = require("./lib/magpie.luau")

local PID_MARK = "__magpie_pid__"
local EXIT_MARK = "__magpie_exit__"
local BACKOFF_MIN = 2
local BACKOFF_MAX = 30
local TICK_IDLE_MS = 30000
local TICK_RETRY_MS = 1000

local generation = 0 -- bumps on every (re)start; lines from older streams are dropped
local running = false
local watchPid = nil
local lastOutput = nil
local backoff = BACKOFF_MIN
local retryAt = nil
local streamCommand = nil

local status = "connecting"
local errorText = nil
local shots = magpie.shots(noctalia.state.get(magpie.STATE_KEY))

local listing = false
local listPending = false

local function now()
  return os.time()
end

local function publish()
  noctalia.state.set(magpie.STATE_KEY, {
    status = status,
    error = errorText,
    shots = shots,
    updated = noctalia.nowMs(),
  })
end

local function limit()
  return tonumber(noctalia.getConfig("list_limit")) or 30
end

-- Re-lists the library. Requests that arrive while a listing is running are
-- folded into one more listing afterwards.
local function refresh()
  if listing then
    listPending = true
    return
  end
  listing = true
  magpie.list(limit(), function(list, err)
    listing = false
    if list then
      shots = list
      if status == "error" and running then
        status = "ready"
        errorText = nil
      end
      -- First run ever: count only shots taken from now on.
      if noctalia.state.get(magpie.SEEN_KEY) == nil then
        magpie.markSeen(list[1] and list[1].id or "")
      end
    else
      status = "error"
      errorText = err
    end
    publish()
    if listPending then
      listPending = false
      refresh()
    end
  end)
end

-- ── Stream ───────────────────────────────────────────────────────────────────

local start -- forward declaration

local function scheduleRetry()
  retryAt = now() + backoff
  backoff = math.min(backoff * 2, BACKOFF_MAX)
  noctalia.setUpdateInterval(TICK_RETRY_MS)
end

local function onStreamEnd(code)
  running = false
  watchPid = nil
  local output = lastOutput or ""
  status = "error"
  if code == 127 or string.find(output, "not found", 1, true) then
    errorText = noctalia.tr("status.command_missing", { command = streamCommand })
  else
    errorText = output ~= "" and output or noctalia.tr("status.stopped")
  end
  publish()
  scheduleRetry()
end

local function onLine(gen, line)
  if gen ~= generation then
    return
  end
  line = magpie.trim(line)
  if line == "" then
    return
  end
  local pid = string.match(line, "^" .. PID_MARK .. " (%d+)$")
  if pid then
    watchPid = pid
    return
  end
  local code = string.match(line, "^" .. EXIT_MARK .. " (%d+)$")
  if code then
    onStreamEnd(tonumber(code))
    return
  end
  if string.sub(line, 1, 1) == "{" then
    local ok, event = pcall(noctalia.json.decode, line)
    if ok and type(event) == "table" then
      if event.event == "ready" then
        backoff = BACKOFF_MIN
        status = "ready"
        errorText = nil
        refresh()
      elseif event.event == "added" or event.event == "changed" then
        refresh()
      elseif event.event == "error" then
        noctalia.log("magpie: " .. tostring(event.message))
      end
      return
    end
  end
  lastOutput = string.sub(line, 1, 500)
end

local function stopStream()
  generation += 1
  if watchPid then
    noctalia.runAsync({ "kill", watchPid })
  end
  watchPid = nil
  running = false
end

start = function()
  stopStream()
  local gen = generation
  streamCommand = magpie.command()
  lastOutput = nil
  retryAt = nil
  running = true
  local script = string.format(
    "exec 2>&1; %s watch & echo %s $!; wait $!; echo %s $?",
    magpie.shellQuote(streamCommand),
    PID_MARK,
    EXIT_MARK
  )
  local ok = noctalia.runStream(script, function(line)
    onLine(gen, line)
  end)
  if ok then
    noctalia.setUpdateInterval(TICK_IDLE_MS)
  else
    running = false
    status = "error"
    errorText = noctalia.tr("status.spawn_failed")
    publish()
    scheduleRetry()
  end
end

local function reconnect()
  backoff = BACKOFF_MIN
  start()
end

-- ── Callbacks ────────────────────────────────────────────────────────────────

function update()
  if not running and retryAt ~= nil and now() >= retryAt then
    start()
  end
end

function onConfigChanged()
  if magpie.command() ~= streamCommand then
    reconnect()
  else
    refresh() -- list_limit may have changed
  end
end

function onIpc(event, payload)
  if event == "refresh" then
    refresh()
  elseif event == "reconnect" then
    reconnect()
  elseif event == "command" then
    noctalia.state.set(magpie.COMMAND_KEY, magpie.trim(payload or ""))
    reconnect()
  end
end

function onExit(_signal, _reason)
  stopStream()
end

-- The panel bumps REFRESH_KEY when it opens. Ignore the value present at load,
-- in case the watch fires once on registration.
local lastRefresh = noctalia.state.get(magpie.REFRESH_KEY)
noctalia.state.watch(magpie.REFRESH_KEY, function(value)
  if value ~= nil and value ~= lastRefresh then
    lastRefresh = value
    refresh()
  end
end)

if noctalia.state.get(magpie.SEEN_KEY) == nil then
  local seen = magpie.loadSeen()
  if seen then
    noctalia.state.set(magpie.SEEN_KEY, seen)
  end
end

publish()
start()
