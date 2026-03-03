#!/usr/bin/env lua
--- hello-moon — tiny demo app tying together moonjson, mooncolor, moontemplate
---
--- What it does:
---   1. Reads a JSON "package database" (inline below)
---   2. Filters packages by a keyword given on the command line
---   3. Renders the results with moontemplate
---   4. Adds color with mooncolor
---
--- Run:
---   lua hello-moon.lua             (list all packages)
---   lua hello-moon.lua json        (filter by keyword)

local json     = require("moonjson")
local color    = require("mooncolor")
local template = require("moontemplate")

-- ── Toy package database (stored as JSON, because why not) ────────────────────

local DB_JSON = [[
[
  {
    "name": "moonjson",
    "version": "0.1.0",
    "description": "A tiny pure-Lua JSON encoder/decoder",
    "license": "MIT",
    "tags": ["json", "serialization", "parsing"]
  },
  {
    "name": "mooncolor",
    "version": "0.1.0",
    "description": "ANSI terminal color and style library",
    "license": "MIT",
    "tags": ["color", "terminal", "ansi", "tty"]
  },
  {
    "name": "moontemplate",
    "version": "0.1.0",
    "description": "Mustache-style template engine with sections and partials",
    "license": "MIT",
    "tags": ["template", "mustache", "html", "rendering"]
  },
  {
    "name": "lua",
    "version": "5.4.7",
    "description": "The Lua programming language runtime",
    "license": "MIT",
    "tags": ["runtime", "interpreter"]
  }
]
]]

-- ── Result template ───────────────────────────────────────────────────────────

local ROW_TMPL = template.compile([[
  {{name}}  {{version}}
    {{description}}
    license: {{license}}  tags: {{tag_str}}
]])

-- ── Search helper ─────────────────────────────────────────────────────────────

local function matches(pkg, keyword)
    if not keyword or keyword == "" then return true end
    local kw = keyword:lower()
    if pkg.name:lower():find(kw, 1, true) then return true end
    if pkg.description:lower():find(kw, 1, true) then return true end
    for _, tag in ipairs(pkg.tags or {}) do
        if tag:lower():find(kw, 1, true) then return true end
    end
    return false
end

-- ── Main ──────────────────────────────────────────────────────────────────────

local keyword = arg[1]

-- Parse the embedded database.
local packages = json.decode(DB_JSON)

-- Filter.
local results = {}
for _, pkg in ipairs(packages) do
    if matches(pkg, keyword) then
        results[#results+1] = pkg
    end
end

-- Header
io.write("\n")
if keyword then
    io.write(color.bold("Search results for ") .. color.cyan(keyword) .. "\n")
else
    io.write(color.bold("All packages") .. "\n")
end
io.write(color.dim(string.rep("─", 60)) .. "\n\n")

-- Render each result.
if #results == 0 then
    io.write(color.warn("No packages found.\n"))
else
    for _, pkg in ipairs(results) do
        -- Build a tag string, colorising each tag individually.
        local colored_tags = {}
        for _, tag in ipairs(pkg.tags or {}) do
            colored_tags[#colored_tags+1] = color.bright_blue(tag)
        end

        local row = ROW_TMPL({
            name        = color.bold(color.green(pkg.name)),
            version     = color.dim(pkg.version),
            description = pkg.description,
            license     = color.yellow(pkg.license),
            tag_str     = table.concat(colored_tags, " "),
        })
        io.write(row .. "\n")
    end
end

-- Footer: demonstrate json.encode round-trip
io.write(color.dim(string.rep("─", 60)) .. "\n")
io.write(
    color.info(string.format(
        "%d package(s) found. Database round-tripped through moonjson:\n",
        #results
    ))
)
-- Re-encode just the names back to JSON as a proof of concept.
local names = {}
for _, p in ipairs(results) do names[#names+1] = p.name end
io.write("  " .. color.cyan(json.encode(names)) .. "\n\n")
