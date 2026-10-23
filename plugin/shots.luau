--!nonstrict
-- Panel `shots`: the screenshot list, newest first. Each shot shows its
-- thumbnail with Copy and Annotate buttons, and its annotated versions
-- indented beneath it in tree order, each with the same two buttons.
-- Annotate opens the editor seeded from that exact version; saving there adds
-- a new version whose parent is the one you started from.
--
-- Opening the panel re-lists the library and clears the bar's "new" count.
--
--   noctalia msg panel-toggle toms/magpie:shots

local magpie = require("./lib/magpie.luau")

local THUMB_W = 128
local THUMB_H = 72
local VARIANT_THUMB_W = 96
local VARIANT_THUMB_H = 54
local INDENT = 24
local MAX_INDENT_DEPTH = 4

local current = magpie.read()
local isOpen = false
local copiedKey = nil -- the row whose Copy button shows a check
local render -- forward declaration

local function copy(key, target)
  magpie.run({ "copy", target }, function(ok)
    if ok then
      copiedKey = key
      render()
    end
  end)
end

local function annotate(target)
  magpie.spawn({ "edit", target })
  panel.close()
end

local function actions(key, target, enabled, annotateTooltip)
  local copied = copiedKey == key
  return {
    ui.button({
      key = key .. "-copy",
      glyph = copied and "check" or "copy",
      glyphSize = 14,
      variant = copied and "secondary" or "ghost",
      enabled = enabled,
      tooltip = noctalia.tr(copied and "panel.copied" or "panel.copy"),
      onClick = function()
        copy(key, target)
      end,
    }),
    ui.button({
      key = key .. "-edit",
      glyph = "pencil",
      glyphSize = 14,
      variant = "ghost",
      enabled = enabled,
      tooltip = annotateTooltip,
      onClick = function()
        annotate(target)
      end,
    }),
  }
end

local function thumb(key, path, width, height)
  if type(path) ~= "string" then
    return ui.box({ key = key .. "-thumb", width = width, height = height, radius = 6, fill = "surface_variant" })
  end
  return ui.image({ key = key .. "-thumb", path = path, width = width, height = height, fit = "cover", radius = 6 })
end

local function shotRow(shot)
  local key = "shot-" .. shot.id
  local count = #(shot.variants or {})
  local subtitle = shot.original_exists and (count > 0 and noctalia.trp("panel.versions", count) or "")
    or noctalia.tr("panel.missing")
  local children = {
    thumb(key, shot.original_exists and (shot.thumb or shot.original) or nil, THUMB_W, THUMB_H),
    ui.column({ flexGrow = 1, gap = 2 }, {
      ui.label({ text = magpie.when(shot.taken_at), fontSize = 13, fontWeight = "medium", maxLines = 1 }),
      ui.label({
        text = subtitle,
        fontSize = 11,
        color = shot.original_exists and "on_surface/0.6" or "error",
        maxLines = 1,
      }),
    }),
  }
  for _, button in ipairs(actions(key, shot.id, shot.original_exists, noctalia.tr("panel.annotate"))) do
    children[#children + 1] = button
  end
  return ui.row({ key = key, gap = 10, align = "center", paddingV = 4 }, children)
end

local function variantRow(shot, entry)
  local variant = entry.variant
  local key = "variant-" .. shot.id .. "-" .. variant.id
  local target = magpie.target(shot.id, variant.id)
  local exists = type(variant.png) == "string" and noctalia.fileExists(variant.png)
  local indent = math.min(entry.depth, MAX_INDENT_DEPTH) * INDENT
  local parent = variant.parent == magpie.ORIGINAL and noctalia.tr("panel.original") or variant.parent
  local children = {
    ui.spacer({ width = indent }),
    ui.glyph({ name = "corner-down-right", size = 14, color = "on_surface/0.4" }),
    thumb(key, exists and (variant.thumb or variant.png) or nil, VARIANT_THUMB_W, VARIANT_THUMB_H),
    ui.column({ flexGrow = 1, gap = 2 }, {
      ui.label({ text = variant.id, fontSize = 12, fontWeight = "medium", maxLines = 1 }),
      ui.label({
        text = noctalia.tr("panel.from", { parent = parent }) .. " · " .. magpie.when(variant.created),
        fontSize = 11,
        color = "on_surface/0.6",
        maxLines = 1,
      }),
    }),
  }
  local tooltip = noctalia.tr("panel.annotate_from", { version = variant.id })
  for _, button in ipairs(actions(key, target, exists, tooltip)) do
    children[#children + 1] = button
  end
  return ui.row({ key = key, gap = 8, align = "center", paddingV = 2 }, children)
end

local function header(shots)
  local count = magpie.newCount(current)
  return ui.row({ align = "center", gap = 8 }, {
    ui.glyph({ name = "camera", size = 18, color = "primary" }),
    ui.label({ text = noctalia.tr("panel.title"), fontSize = 16, fontWeight = "bold" }),
    ui.label({
      text = count > 0 and noctalia.tr("widget.tooltip_new", { count = count }) or tostring(#shots),
      fontSize = 11,
      color = "on_surface/0.6",
      flexGrow = 1,
      maxLines = 1,
    }),
    ui.button({
      glyph = "refresh",
      glyphSize = 14,
      variant = "ghost",
      tooltip = noctalia.tr("panel.refresh"),
      onClick = "onRefreshClicked",
    }),
    ui.button({
      glyph = "x",
      glyphSize = 14,
      variant = "ghost",
      tooltip = noctalia.tr("panel.close"),
      onClick = "onCloseClicked",
    }),
  })
end

local function banner(text)
  return ui.row({ gap = 8, padding = 10, radius = 8, align = "center", fill = "surface_variant/0.5" }, {
    ui.glyph({ name = "alert-triangle", size = 16, color = "error" }),
    ui.label({ text = text, fontSize = 12, flexGrow = 1, maxLines = 3 }),
  })
end

local function build()
  local shots = magpie.shots(current)
  local children = { header(shots) }
  if current.status == "error" then
    children[#children + 1] = banner(tostring(current.error or noctalia.tr("status.stopped")))
  end
  if #shots == 0 then
    children[#children + 1] = ui.label({
      text = current.status == "connecting" and noctalia.tr("status.connecting") or noctalia.tr("panel.empty"),
      fontSize = 12,
      color = "on_surface/0.6",
      maxLines = 3,
    })
    return ui.column({ flexGrow = 1, gap = 10 }, children)
  end

  local rows = {}
  for index, shot in ipairs(shots) do
    if index > 1 then
      rows[#rows + 1] = ui.separator({ key = "sep-" .. shot.id })
    end
    rows[#rows + 1] = shotRow(shot)
    for _, entry in ipairs(magpie.variantTree(shot)) do
      rows[#rows + 1] = variantRow(shot, entry)
    end
  end
  children[#children + 1] = ui.scroll({ flexGrow = 1, gap = 4 }, rows)
  return ui.column({ flexGrow = 1, gap = 10 }, children)
end

render = function()
  if isOpen then
    magpie.renderPanel(build)
  end
end

function onOpen(_context)
  isOpen = true
  copiedKey = nil
  current = magpie.read()
  magpie.requestRefresh()
  render()
end

function onClose()
  isOpen = false
  local newest = magpie.shots(current)[1]
  if newest then
    magpie.markSeen(newest.id)
  end
end

function onRefreshClicked()
  magpie.requestRefresh()
end

function onCloseClicked()
  panel.close()
end

function onIpc(_event, _payload) end

noctalia.state.watch(magpie.STATE_KEY, function(value)
  current = type(value) == "table" and value or magpie.read()
  render()
end)
