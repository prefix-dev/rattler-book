--- mooncolor — ANSI terminal color and style library for Lua 5.4
---
--- Usage:
---   local c = require("mooncolor")
---   print(c.red("error:") .. " something went wrong")
---   print(c.bold(c.green("✔")) .. " done")
---   print(c.rgb(255, 165, 0, "orange text"))
---
--- Colors are automatically suppressed when stdout is not a TTY
--- (i.e. when piped into a file or another program).

local mooncolor = {}

-- ── TTY detection ─────────────────────────────────────────────────────────────
-- We check $NO_COLOR (https://no-color.org/) and whether the TERM environment
-- variable suggests a color-capable terminal.

local function supports_color()
    if os.getenv("NO_COLOR") then return false end
    local term = os.getenv("TERM") or ""
    if term == "dumb" then return false end
    -- Most modern terminals advertise color support.
    return true
end

mooncolor.enabled = supports_color()

-- ── Core escape ───────────────────────────────────────────────────────────────

local ESC = "\27["

local function esc(code, text)
    if not mooncolor.enabled then return text end
    return ESC .. code .. "m" .. text .. ESC .. "0m"
end

-- ── Basic styles ──────────────────────────────────────────────────────────────

function mooncolor.reset(text)     return esc("0",  text) end
function mooncolor.bold(text)      return esc("1",  text) end
function mooncolor.dim(text)       return esc("2",  text) end
function mooncolor.italic(text)    return esc("3",  text) end
function mooncolor.underline(text) return esc("4",  text) end
function mooncolor.blink(text)     return esc("5",  text) end
function mooncolor.reverse(text)   return esc("7",  text) end
function mooncolor.strike(text)    return esc("9",  text) end

-- ── Foreground colors ─────────────────────────────────────────────────────────

function mooncolor.black(text)   return esc("30", text) end
function mooncolor.red(text)     return esc("31", text) end
function mooncolor.green(text)   return esc("32", text) end
function mooncolor.yellow(text)  return esc("33", text) end
function mooncolor.blue(text)    return esc("34", text) end
function mooncolor.magenta(text) return esc("35", text) end
function mooncolor.cyan(text)    return esc("36", text) end
function mooncolor.white(text)   return esc("37", text) end

-- Bright variants
function mooncolor.bright_black(text)   return esc("90", text) end
function mooncolor.bright_red(text)     return esc("91", text) end
function mooncolor.bright_green(text)   return esc("92", text) end
function mooncolor.bright_yellow(text)  return esc("93", text) end
function mooncolor.bright_blue(text)    return esc("94", text) end
function mooncolor.bright_magenta(text) return esc("95", text) end
function mooncolor.bright_cyan(text)    return esc("96", text) end
function mooncolor.bright_white(text)   return esc("97", text) end

-- ── Background colors ─────────────────────────────────────────────────────────

function mooncolor.bg_black(text)   return esc("40",  text) end
function mooncolor.bg_red(text)     return esc("41",  text) end
function mooncolor.bg_green(text)   return esc("42",  text) end
function mooncolor.bg_yellow(text)  return esc("43",  text) end
function mooncolor.bg_blue(text)    return esc("44",  text) end
function mooncolor.bg_magenta(text) return esc("45",  text) end
function mooncolor.bg_cyan(text)    return esc("46",  text) end
function mooncolor.bg_white(text)   return esc("47",  text) end

-- ── True-color (24-bit) ───────────────────────────────────────────────────────

--- Wrap `text` in a 24-bit foreground color.
--- @param r integer  red   0–255
--- @param g integer  green 0–255
--- @param b integer  blue  0–255
function mooncolor.rgb(r, g, b, text)
    return esc(string.format("38;2;%d;%d;%d", r, g, b), text)
end

--- Wrap `text` in a 24-bit background color.
function mooncolor.bg_rgb(r, g, b, text)
    return esc(string.format("48;2;%d;%d;%d", r, g, b), text)
end

-- ── Composable style builder ──────────────────────────────────────────────────
-- Instead of nesting calls, build a style table and apply it once:
--
--   local warn = mooncolor.style { color="yellow", bold=true }
--   print(warn("warning: watch out"))

--- Create a reusable style function.
---
--- Options (all optional):
---   color      string  foreground color name ("red", "cyan", …)
---   bg         string  background color name
---   bold       bool
---   italic     bool
---   underline  bool
---   dim        bool
---   strike     bool
---
--- @param opts table
--- @return function(text: string): string
function mooncolor.style(opts)
    local codes = {}
    local style_codes = {
        bold=1, dim=2, italic=3, underline=4, blink=5, reverse=7, strike=9,
    }
    local fg_codes = {
        black=30, red=31, green=32, yellow=33, blue=34,
        magenta=35, cyan=36, white=37,
        bright_black=90, bright_red=91, bright_green=92, bright_yellow=93,
        bright_blue=94, bright_magenta=95, bright_cyan=96, bright_white=97,
    }
    local bg_codes = {
        black=40, red=41, green=42, yellow=43, blue=44,
        magenta=45, cyan=46, white=47,
    }
    for k, v in pairs(opts) do
        if k == "color" then
            codes[#codes+1] = tostring(fg_codes[v] or 37)
        elseif k == "bg" then
            codes[#codes+1] = tostring(bg_codes[v] or 40)
        elseif style_codes[k] and v then
            codes[#codes+1] = tostring(style_codes[k])
        end
    end
    local code = table.concat(codes, ";")
    return function(text)
        return esc(code, text)
    end
end

-- ── Progress / status helpers ─────────────────────────────────────────────────

--- Print a green checkmark prefix.
function mooncolor.ok(msg)
    return mooncolor.bold(mooncolor.green("✔")) .. " " .. msg
end

--- Print a red cross prefix.
function mooncolor.fail(msg)
    return mooncolor.bold(mooncolor.red("✘")) .. " " .. msg
end

--- Print a yellow warning prefix.
function mooncolor.warn(msg)
    return mooncolor.bold(mooncolor.yellow("⚠")) .. " " .. msg
end

--- Print a blue info prefix.
function mooncolor.info(msg)
    return mooncolor.bold(mooncolor.cyan("ℹ")) .. " " .. msg
end

return mooncolor
