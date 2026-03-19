-- ~/~ begin <<book/src/ch09-build.md#src/build_prelude.lua>>[init]
-- ~/~ begin <<book/src/ch09-build.md#prelude-header>>[init]
-- moonshot build prelude
-- Automatically sourced before every build.lua by `shot build`.
--
-- You do NOT need to require() this file; everything below is already
-- in scope when your build script runs.
--
-- Available globals
-- -----------------
--   PREFIX        Where the package should be installed
--   SRC_DIR       Root of your source tree
--   BUILD_PREFIX  Where build-time dependencies live (e.g. lua itself)
--   PKG_NAME      Package name from recipe.toml
--   PKG_VERSION   Package version from recipe.toml
--   PKG_BUILD_NUM Build number (integer)
--
-- Available functions
-- -------------------
--   mkdir(path)
--   cp(src, dst)
--   mv(src, dst)
--   install(src, subdir)      copies src into PREFIX/subdir/
--   install_bin(src)          copies src into PREFIX/bin/
--   install_lua(src, ver)     copies src into PREFIX/share/lua/<ver>/
--   install_lib(src)          copies src into PREFIX/lib/
--   install_share(src, name)  copies src into PREFIX/share/<name>/
--   path_join(...)            joins path segments with "/"
--   exists(path)              returns true if path exists
--   is_file(path)             returns true if path is a regular file
--   log(msg)                  prints "[moonshot] msg" to stderr
-- ~/~ end

-- ~/~ begin <<book/src/ch09-build.md#prelude-globals>>[init]
-- ── Globals ───────────────────────────────────────────────────────────────────

PREFIX        = os.getenv("PREFIX")        or error("PREFIX not set")
SRC_DIR       = os.getenv("SRC_DIR")       or error("SRC_DIR not set")
BUILD_PREFIX  = os.getenv("BUILD_PREFIX")  or error("BUILD_PREFIX not set")
PKG_NAME      = os.getenv("PKG_NAME")      or error("PKG_NAME not set")
PKG_VERSION   = os.getenv("PKG_VERSION")   or error("PKG_VERSION not set")
PKG_BUILD_NUM = tonumber(os.getenv("PKG_BUILD_NUM") or "0")
-- ~/~ end

-- ~/~ begin <<book/src/ch09-build.md#prelude-internal-helpers>>[init]
-- ── Internal helpers ──────────────────────────────────────────────────────────

local function shell(cmd)
    local ok, kind, code = os.execute(cmd)
    if not ok then
        error(string.format("Command failed (exit %d):\n  %s", code or -1, cmd), 2)
    end
end

-- Quote a path for use in a shell command.
local function q(path)
    -- Wrap in single quotes; escape any embedded single quotes.
    return "'" .. path:gsub("'", "'\\''") .. "'"
end
-- ~/~ end

-- ~/~ begin <<book/src/ch09-build.md#prelude-public-api>>[init]
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
    shell("mkdir -p " .. q(path))
end

--- Copy `src` to `dst`.  `src` may contain shell globs.
function cp(src, dst)
    mkdir(dst)
    shell("cp -r " .. src .. " " .. q(dst))
end

--- Move `src` to `dst`.
function mv(src, dst)
    shell("mv " .. q(src) .. " " .. q(dst))
end

--- Return true if `path` exists.
function exists(path)
    local f = io.open(path, "r")
    if f then f:close(); return true end
    return false
end

--- Return true if `path` is a regular file.
function is_file(path)
    -- Portable check: open for reading succeeds only for files.
    local f = io.open(path, "rb")
    if f then f:close(); return true end
    return false
end

--- Print an informational message to stderr, prefixed with "[moonshot]".
function log(msg)
    io.stderr:write("[moonshot] " .. tostring(msg) .. "\n")
end
-- ~/~ end

-- ~/~ begin <<book/src/ch09-build.md#prelude-install-helpers>>[init]
-- ── Install helpers ───────────────────────────────────────────────────────────

--- Install files matching `src` (a path or shell glob) into `PREFIX/subdir/`.
---
--- Example:
---   install("*.lua", "share/lua/5.4")
---   install("src/mylib/*.lua", "share/lua/5.4")
function install(src, subdir)
    local dst = path_join(PREFIX, subdir)
    mkdir(dst)
    -- Expand src relative to SRC_DIR if it is not absolute.
    local expanded = src:sub(1,1) == "/" and src or path_join(SRC_DIR, src)
    shell("cp -r " .. expanded .. " " .. q(dst) .. "/")
end

--- Install an executable into `PREFIX/bin/`.
---
--- Example:
---   install_bin("bin/mylua")
function install_bin(src)
    local dst = path_join(PREFIX, "bin")
    mkdir(dst)
    local expanded = src:sub(1,1) == "/" and src or path_join(SRC_DIR, src)
    shell("cp " .. expanded .. " " .. q(dst) .. "/")
    -- Make the installed file executable.
    local base = src:match("[^/]+$")
    shell("chmod +x " .. q(path_join(dst, base)))
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

--- Install files into `PREFIX/lib/`.
---
--- Example:
---   install_lib("build/*.so")
function install_lib(src)
    install(src, "lib")
end

--- Install files into `PREFIX/share/<name>/`.
---
--- Example:
---   install_share("docs/", "moonshine")
function install_share(src, name)
    install(src, path_join("share", name))
end
-- ~/~ end

-- ~/~ begin <<book/src/ch09-build.md#prelude-done>>[init]
-- ── Done ──────────────────────────────────────────────────────────────────────

log(string.format("Building %s %s (build %d)", PKG_NAME, PKG_VERSION, PKG_BUILD_NUM))
log(string.format("PREFIX    = %s", PREFIX))
log(string.format("SRC_DIR   = %s", SRC_DIR))
-- ~/~ end
-- ~/~ end
