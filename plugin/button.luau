--!nonstrict
-- Bar widget `button`: camera glyph, plus the number of screenshots taken
-- since the panel was last opened. Turns primary when there are new ones and
-- red when `magpie watch` is not running.
--
-- Left click toggles the shots panel; right click copies the newest shot.
--
--   noctalia msg plugin toms/magpie:button focused probe

local magpie = require("./lib/magpie.luau")

local current = magpie.read()

local function render()
  barWidget.setGlyph("camera")
  local count = magpie.newCount(current)
  if current.status == "error" then
    barWidget.setGlyphColor("error")
    barWidget.setText("")
    barWidget.setTooltip(tostring(current.error or noctalia.tr("status.stopped")))
    return
  end
  barWidget.setGlyphColor(count > 0 and "primary" or "on_surface")
  barWidget.setColor(count > 0 and "primary" or "on_surface")
  barWidget.setText(count > 0 and tostring(count) or "")
  if count > 0 then
    barWidget.setTooltip(noctalia.tr("widget.tooltip_new", { count = count }))
  else
    barWidget.setTooltip(noctalia.tr("widget.tooltip"))
  end
end

function onClick()
  noctalia.togglePanel(magpie.PANEL)
end

function onRightClick()
  local newest = magpie.shots(current)[1]
  if newest and newest.original_exists then
    magpie.run({ "copy", newest.id }, function(ok)
      if ok then
        noctalia.notify(noctalia.tr("widget.copied"), magpie.when(newest.taken_at))
      end
    end)
  end
end

function onIpc(_event, _payload) end

noctalia.state.watch(magpie.STATE_KEY, function(value)
  current = type(value) == "table" and value or magpie.read()
  render()
end)

noctalia.state.watch(magpie.SEEN_KEY, function(_value)
  render()
end)

render()
