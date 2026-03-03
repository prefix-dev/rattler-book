--- moonjson — a tiny JSON encoder/decoder for Lua 5.4
--- Pure Lua, no dependencies.
---
--- Usage:
---   local json = require("moonjson")
---   local s = json.encode({ name = "rattler", version = "1.0" })
---   local t = json.decode(s)
---   print(t.name)   --> rattler

local moonjson = {}

-- ── Encoder ──────────────────────────────────────────────────────────────────

local ESC = {
    ['"']  = '\\"',
    ['\\'] = '\\\\',
    ['\n'] = '\\n',
    ['\r'] = '\\r',
    ['\t'] = '\\t',
    ['\b'] = '\\b',
    ['\f'] = '\\f',
}

local function encode_string(s)
    return '"' .. s:gsub('[\\"/%c]', function(c)
        return ESC[c] or string.format('\\u%04x', c:byte())
    end) .. '"'
end

local function encode_value(val, indent, level)
    local t = type(val)

    if t == "nil" then
        return "null"
    elseif t == "boolean" then
        return tostring(val)
    elseif t == "number" then
        if val ~= val then return "null" end          -- NaN
        if val == math.huge or val == -math.huge then  -- Inf
            return "null"
        end
        -- Emit integers without a decimal point.
        if math.type(val) == "integer" then
            return string.format("%d", val)
        end
        return string.format("%.17g", val)
    elseif t == "string" then
        return encode_string(val)
    elseif t == "table" then
        -- Decide: array or object?
        local is_array = true
        local max_n = 0
        for k, _ in pairs(val) do
            if type(k) ~= "number" or k < 1 or math.floor(k) ~= k then
                is_array = false
                break
            end
            if k > max_n then max_n = k end
        end
        -- Sparse tables (with holes) are encoded as objects.
        if is_array and max_n ~= #val then is_array = false end

        local parts = {}
        if indent then
            local pad     = string.rep(indent, level + 1)
            local pad_end = string.rep(indent, level)
            if is_array then
                for i = 1, #val do
                    parts[i] = pad .. encode_value(val[i], indent, level + 1)
                end
                return "[\n" .. table.concat(parts, ",\n") .. "\n" .. pad_end .. "]"
            else
                local keys = {}
                for k in pairs(val) do keys[#keys+1] = k end
                table.sort(keys, function(a, b)
                    return tostring(a) < tostring(b)
                end)
                for _, k in ipairs(keys) do
                    parts[#parts+1] = pad .. encode_string(tostring(k))
                                   .. ": " .. encode_value(val[k], indent, level + 1)
                end
                return "{\n" .. table.concat(parts, ",\n") .. "\n" .. pad_end .. "}"
            end
        else
            if is_array then
                for i = 1, #val do
                    parts[i] = encode_value(val[i])
                end
                return "[" .. table.concat(parts, ",") .. "]"
            else
                local keys = {}
                for k in pairs(val) do keys[#keys+1] = k end
                table.sort(keys, function(a, b)
                    return tostring(a) < tostring(b)
                end)
                for _, k in ipairs(keys) do
                    parts[#parts+1] = encode_string(tostring(k))
                                   .. ":" .. encode_value(val[k])
                end
                return "{" .. table.concat(parts, ",") .. "}"
            end
        end
    else
        error("cannot encode value of type " .. t, 2)
    end
end

--- Encode `value` as a JSON string.
---
--- @param value  any Lua value (nil, bool, number, string, table)
--- @param indent string|nil  indent string for pretty-printing (e.g. "  ")
--- @return string
function moonjson.encode(value, indent)
    return encode_value(value, indent, 0)
end

-- ── Decoder ──────────────────────────────────────────────────────────────────

local function make_parser(s)
    local pos = 1

    local function err(msg)
        error(string.format("JSON parse error at position %d: %s", pos, msg), 2)
    end

    local function skip_ws()
        pos = s:match("^%s*()", pos)
    end

    local function peek()
        skip_ws()
        return s:sub(pos, pos)
    end

    local function consume(ch)
        skip_ws()
        if s:sub(pos, pos) ~= ch then
            err(string.format("expected %q, got %q", ch, s:sub(pos, pos)))
        end
        pos = pos + 1
    end

    local parse_value  -- forward declaration

    local UNESCAPE = {
        ['"']  = '"', ['\\'] = '\\', ['/'] = '/',
        ['n']  = '\n', ['r'] = '\r', ['t'] = '\t',
        ['b']  = '\b', ['f'] = '\f',
    }

    local function parse_string()
        consume('"')
        local parts = {}
        while true do
            local plain, esc = s:match('^([^"\\]*)(["\\])', pos)
            if not plain then err("unterminated string") end
            if plain ~= "" then parts[#parts+1] = plain end
            pos = pos + #plain + 1
            if esc == '"' then break end
            -- handle escape
            local ec = s:sub(pos, pos)
            pos = pos + 1
            if ec == 'u' then
                local hex = s:match('^%x%x%x%x', pos)
                if not hex then err("invalid \\u escape") end
                pos = pos + 4
                local cp = tonumber(hex, 16)
                -- Basic BMP only (good enough for our purposes)
                parts[#parts+1] = utf8.char(cp)
            else
                local ch = UNESCAPE[ec]
                if not ch then err("invalid escape \\" .. ec) end
                parts[#parts+1] = ch
            end
        end
        return table.concat(parts)
    end

    local function parse_array()
        consume('[')
        local arr = {}
        skip_ws()
        if s:sub(pos, pos) == ']' then pos = pos + 1; return arr end
        while true do
            arr[#arr+1] = parse_value()
            skip_ws()
            local c = s:sub(pos, pos)
            if c == ']' then pos = pos + 1; break
            elseif c == ',' then pos = pos + 1
            else err("expected ',' or ']'") end
        end
        return arr
    end

    local function parse_object()
        consume('{')
        local obj = {}
        skip_ws()
        if s:sub(pos, pos) == '}' then pos = pos + 1; return obj end
        while true do
            skip_ws()
            if s:sub(pos, pos) ~= '"' then err("expected string key") end
            local key = parse_string()
            consume(':')
            obj[key] = parse_value()
            skip_ws()
            local c = s:sub(pos, pos)
            if c == '}' then pos = pos + 1; break
            elseif c == ',' then pos = pos + 1
            else err("expected ',' or '}'") end
        end
        return obj
    end

    parse_value = function()
        skip_ws()
        local c = s:sub(pos, pos)
        if c == '"' then
            return parse_string()
        elseif c == '[' then
            return parse_array()
        elseif c == '{' then
            return parse_object()
        elseif s:match('^true', pos) then
            pos = pos + 4; return true
        elseif s:match('^false', pos) then
            pos = pos + 5; return false
        elseif s:match('^null', pos) then
            pos = pos + 4; return nil
        else
            -- number
            local num_str = s:match('^-?%d+%.?%d*[eE]?[+-]?%d*', pos)
            if not num_str then err("unexpected token: " .. c) end
            pos = pos + #num_str
            return tonumber(num_str)
        end
    end

    return parse_value, function() return pos end
end

--- Decode a JSON string into a Lua value.
---
--- JSON null becomes nil inside tables (the key is absent from the table).
--- JSON arrays become Lua sequences; JSON objects become tables.
---
--- @param s string   JSON text
--- @return any
function moonjson.decode(s)
    local parse_value = make_parser(s)
    return parse_value()
end

return moonjson
