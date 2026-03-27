-- ~/~ begin <<book/src/ch10-build.md#src/build_prelude.lua>>[init]
-- ~/~ begin <<book/src/ch10-build.md#prelude-header>>[init]
-- moonshot build prelude
-- Automatically sourced before every build.lua by `shot build`.
-- ~/~ end

-- ~/~ begin <<book/src/ch10-build.md#prelude-globals>>[init]
-- ── Globals ───────────────────────────────────────────────────────────────────

PREFIX        = os.getenv("PREFIX")        or error("PREFIX not set")
SRC_DIR       = os.getenv("SRC_DIR")       or error("SRC_DIR not set")
BUILD_PREFIX  = os.getenv("BUILD_PREFIX")  or error("BUILD_PREFIX not set")
PKG_NAME      = os.getenv("PKG_NAME")      or error("PKG_NAME not set")
PKG_VERSION   = os.getenv("PKG_VERSION")   or error("PKG_VERSION not set")
PKG_BUILD_NUM = tonumber(os.getenv("PKG_BUILD_NUM") or "0")
-- ~/~ end

-- ~/~ begin <<book/src/ch10-build.md#prelude-internal-helpers>>[init]
-- ── Internal helpers ──────────────────────────────────────────────────────────

--- Detect the operating system once.  `package.config` always starts with the
--- directory separator (`\` on Windows, `/` everywhere else).
local IS_WINDOWS = package.config:sub(1, 1) == "\\"

local function shell(cmd)
    local ok, kind, code = os.execute(cmd)
    if not ok then
        error(string.format("Command failed (exit %d):\n  %s", code or -1, cmd), 2)
    end
end

-- Quote a path for use in a shell command.
local function q(path)
    if IS_WINDOWS then
        -- cmd.exe uses double-quote delimiters; normalise to backslashes.
        return '"' .. path:gsub("/", "\\") .. '"'
    else
        -- POSIX: wrap in single quotes; escape any embedded single quotes.
        return "'" .. path:gsub("'", "'\\''") .. "'"
    end
end
-- ~/~ end

-- ~/~ begin <<book/src/ch10-build.md#prelude-public-api>>[init]
-- ── Public API ────────────────────────────────────────────────────────────────

--- Join path segments with "/", collapsing duplicate slashes.
function path_join(...)
    local parts = {...}
    local result = table.concat(parts, "/")
    -- collapse double slashes (but keep leading "//", used on some POSIX systems)
    result = result:gsub("([^:])//+", "%1/")
    return result
end

--- Create `path` and all parent directories (like `mkdir -p`).
function mkdir(path)
    if IS_WINDOWS then
        -- Windows mkdir creates parent directories by default.
        -- Suppress the "already exists" error with 2>nul.
        os.execute("mkdir " .. q(path) .. " 2>nul")
    else
        shell("mkdir -p " .. q(path))
    end
end

--- Copy `src` to `dst`.  `src` may contain shell globs.
function cp(src, dst)
    mkdir(dst)
    if IS_WINDOWS then
        shell("copy /Y " .. q(src) .. " " .. q(dst))
    else
        shell("cp -r " .. src .. " " .. q(dst))
    end
end

--- Print an informational message to stderr, prefixed with "[moonshot]".
function log(msg)
    io.stderr:write("[moonshot] " .. tostring(msg) .. "\n")
end
-- ~/~ end

-- ~/~ begin <<book/src/ch10-build.md#prelude-install-helpers>>[init]
-- ── Install helpers ───────────────────────────────────────────────────────────

--- Return true when `src` is an absolute path on the current platform.
local function is_absolute(src)
    if src:sub(1, 1) == "/" then return true end
    -- Windows drive letter, e.g. "C:\" or "C:/"
    if IS_WINDOWS and src:match("^%a:[/\\]") then return true end
    return false
end

--- Install files matching `src` (a path or shell glob) into `PREFIX/subdir/`.
---
--- Example:
---   install("*.lua", "share/lua/5.4")
---   install("src/mylib/*.lua", "share/lua/5.4")
function install(src, subdir)
    local dst = path_join(PREFIX, subdir)
    mkdir(dst)
    -- Expand src relative to SRC_DIR if it is not absolute.
    local expanded = is_absolute(src) and src or path_join(SRC_DIR, src)
    if IS_WINDOWS then
        shell("copy /Y " .. q(expanded) .. " " .. q(dst))
    else
        shell("cp -r " .. expanded .. " " .. q(dst) .. "/")
    end
end

--- Install an executable into `PREFIX/bin/`.
---
--- Example:
---   install_bin("bin/mylua")
function install_bin(src)
    local dst = path_join(PREFIX, "bin")
    mkdir(dst)
    local expanded = is_absolute(src) and src or path_join(SRC_DIR, src)
    if IS_WINDOWS then
        shell("copy /Y " .. q(expanded) .. " " .. q(dst))
    else
        shell("cp " .. expanded .. " " .. q(dst) .. "/")
        -- Make the installed file executable.
        local base = src:match("[^/]+$")
        shell("chmod +x " .. q(path_join(dst, base)))
    end
end

--- Install Lua source files into the standard Lua package path.
---
--- `ver` defaults to "5.4".
---
--- After installation a user can write:
---   local mylib = require("mylib")
---
--- Example:
---   install_lua("*.lua")           -- → PREFIX/share/lua/5.4/
---   install_lua("src/*.lua", "5.1") -- → PREFIX/share/lua/5.1/
function install_lua(src, ver)
    ver = ver or "5.4"
    install(src, path_join("share", "lua", ver))
end

-- ~/~ end

-- ~/~ begin <<book/src/ch10-build.md#prelude-done>>[init]
-- ── Done ──────────────────────────────────────────────────────────────────────

log(string.format("Building %s %s (build %d)", PKG_NAME, PKG_VERSION, PKG_BUILD_NUM))
log(string.format("PREFIX    = %s", PREFIX))
log(string.format("SRC_DIR   = %s", SRC_DIR))
-- ~/~ end
-- ~/~ end
