--- moontemplate — a lightweight template engine for Lua 5.4
---
--- Supports:
---   {{var}}          substitute variable (HTML-escaped)
---   {{{var}}}        substitute variable (raw, no escaping)
---   {{#section}}     render block only if value is truthy / iterate list
---   {{/section}}     close section
---   {{^section}}     render block only if value is falsy (inverted section)
---   {{! comment }}   ignored
---   {{> partial}}    include another template by name
---
--- Usage:
---   local mt = require("moontemplate")
---
---   local tmpl = mt.compile([[
---     Hello, {{name}}!
---     {{#items}}
---       - {{{label}}}
---     {{/items}}
---   ]])
---   print(tmpl({ name = "world", items = {
---     { label = "<b>bold</b>" },
---     { label = "plain" },
---   }}))

local moontemplate = {}

-- ── HTML escaping ─────────────────────────────────────────────────────────────

local HTML_ESC = {
    ['&']  = '&amp;',
    ['<']  = '&lt;',
    ['>']  = '&gt;',
    ['"']  = '&quot;',
    ["'"]  = '&#39;',
}

local function escape(s)
    return (tostring(s):gsub('[&<>"\']', HTML_ESC))
end

-- ── Context stack lookup ──────────────────────────────────────────────────────
-- When rendering inside a {{#section}}, the context chain is a list of tables
-- checked from innermost to outermost — just like Mustache spec requires.

local function lookup(stack, key)
    if key == "." then return stack[#stack] end
    for i = #stack, 1, -1 do
        local ctx = stack[i]
        if type(ctx) == "table" then
            local v = ctx[key]
            if v ~= nil then return v end
        end
    end
    return nil
end

-- ── Tokeniser ─────────────────────────────────────────────────────────────────
-- We scan for {{ ... }} pairs and emit a flat list of token tables.

local function tokenise(src)
    local tokens = {}
    local pos = 1
    local len = #src

    while pos <= len do
        local s, e = src:find("{{", pos, true)
        if not s then
            -- trailing literal
            tokens[#tokens+1] = { kind = "text", value = src:sub(pos) }
            break
        end
        if s > pos then
            tokens[#tokens+1] = { kind = "text", value = src:sub(pos, s - 1) }
        end

        -- Is it a triple-stache?
        local triple = src:sub(e + 1, e + 1) == "{"
        local close = triple and "}}}" or "}}"
        local inner_start = triple and e + 2 or e + 1
        local ce, cee = src:find(close, inner_start, true)
        if not ce then error("unclosed tag at position " .. s) end

        local tag = src:sub(inner_start, ce - 1):match("^%s*(.-)%s*$")
        pos = cee + 1

        local first = tag:sub(1, 1)
        if first == '!' then
            -- comment — skip
        elseif first == '#' then
            tokens[#tokens+1] = { kind = "open",     value = tag:sub(2):match("^%s*(.-)%s*$") }
        elseif first == '^' then
            tokens[#tokens+1] = { kind = "inverted", value = tag:sub(2):match("^%s*(.-)%s*$") }
        elseif first == '/' then
            tokens[#tokens+1] = { kind = "close",    value = tag:sub(2):match("^%s*(.-)%s*$") }
        elseif first == '>' then
            tokens[#tokens+1] = { kind = "partial",  value = tag:sub(2):match("^%s*(.-)%s*$") }
        elseif triple then
            tokens[#tokens+1] = { kind = "raw",      value = tag }
        else
            tokens[#tokens+1] = { kind = "var",      value = tag }
        end
    end

    return tokens
end

-- ── Renderer ──────────────────────────────────────────────────────────────────

local function render_tokens(tokens, i, stack, partials, out)
    i = i or 1
    while i <= #tokens do
        local tok = tokens[i]

        if tok.kind == "text" then
            out[#out+1] = tok.value
            i = i + 1

        elseif tok.kind == "var" then
            local v = lookup(stack, tok.value)
            if v ~= nil then
                out[#out+1] = escape(v)
            end
            i = i + 1

        elseif tok.kind == "raw" then
            local v = lookup(stack, tok.value)
            if v ~= nil then
                out[#out+1] = tostring(v)
            end
            i = i + 1

        elseif tok.kind == "open" then
            local name = tok.value
            local v    = lookup(stack, name)
            i = i + 1
            -- collect the inner token range
            local inner_start = i
            local depth = 1
            while i <= #tokens do
                if tokens[i].kind == "open" and tokens[i].value == name then
                    depth = depth + 1
                elseif tokens[i].kind == "close" and tokens[i].value == name then
                    depth = depth - 1
                    if depth == 0 then break end
                end
                i = i + 1
            end
            local inner_end = i - 1
            i = i + 1  -- consume the closing tag

            if v then
                if type(v) == "table" and #v > 0 then
                    -- iterate the list
                    for _, item in ipairs(v) do
                        stack[#stack+1] = item
                        render_tokens(tokens, inner_start, stack, partials, out)
                        stack[#stack] = nil
                    end
                else
                    -- truthy scalar / object — render once with v pushed
                    stack[#stack+1] = (type(v) == "table") and v or {}
                    render_tokens(tokens, inner_start, stack, partials, out)
                    stack[#stack] = nil
                end
            end
            -- falsy → skip

        elseif tok.kind == "inverted" then
            local name = tok.value
            local v    = lookup(stack, name)
            i = i + 1
            local inner_start = i
            local depth = 1
            while i <= #tokens do
                if tokens[i].kind == "inverted" and tokens[i].value == name then
                    depth = depth + 1
                elseif tokens[i].kind == "close" and tokens[i].value == name then
                    depth = depth - 1
                    if depth == 0 then break end
                end
                i = i + 1
            end
            local inner_end = i - 1
            i = i + 1

            if not v or (type(v) == "table" and #v == 0) then
                render_tokens(tokens, inner_start, stack, partials, out)
            end

        elseif tok.kind == "close" then
            -- handled inside open/inverted; if we see one here it's an error
            error("unexpected closing tag {{/" .. tok.value .. "}}")

        elseif tok.kind == "partial" then
            local src = partials and partials[tok.value]
            if src then
                -- Partials inherit the current context stack.
                local sub_tokens = tokenise(src)
                render_tokens(sub_tokens, 1, stack, partials, out)
            end
            i = i + 1
        end
    end
end

-- ── Public API ────────────────────────────────────────────────────────────────

--- Compile a template string into a reusable render function.
---
--- @param src      string   template source
--- @param partials table?   map of partial name → template string
--- @return function(ctx: table): string
function moontemplate.compile(src, partials)
    local tokens = tokenise(src)
    return function(ctx)
        local out   = {}
        local stack = { ctx }
        render_tokens(tokens, 1, stack, partials, out)
        return table.concat(out)
    end
end

--- Render a template string with a context in one step.
---
--- @param src      string
--- @param ctx      table
--- @param partials table?
--- @return string
function moontemplate.render(src, ctx, partials)
    return moontemplate.compile(src, partials)(ctx)
end

return moontemplate
