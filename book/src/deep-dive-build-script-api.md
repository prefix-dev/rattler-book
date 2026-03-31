# Deep Dive: The Build Script API

When `shot build` runs your `build.lua`, it first loads a prelude that sets up
globals and helper functions. This chapter walks through the full prelude,
explaining each piece along the way.

Here is the complete file skeleton for `src/build_prelude.lua`:

``` {.lua file=src/build_prelude.lua}
<<prelude-header>>

<<prelude-globals>>

<<prelude-internal-helpers>>

<<prelude-public-api>>

<<prelude-install-helpers>>

<<prelude-done>>
```

``` {.lua #prelude-header}
-- moonshot build prelude
-- Automatically sourced before every build.lua by `shot build`.
```

## Globals

The Rust side sets environment variables before launching the Lua interpreter.
The prelude reads them once and fails fast if any are missing:

| Global | Source | Meaning |
|---|---|---|
| **PREFIX** | `--install-prefix` temp dir | Where the package should install files |
| **SRC_DIR** | current working directory | Root of your source tree |
| **BUILD_PREFIX** | `--build-prefix` temp dir | Where build dependencies live |
| **PKG_NAME** | `moonshot.toml [project].name` | Package name |
| **PKG_VERSION** | `moonshot.toml [project].version` | Package version |
| **PKG_BUILD_NUM** | `moonshot.toml [build].build_number` | Build number (integer) |

`PREFIX` and `BUILD_PREFIX` are temporary directories created by `shot build`.
Files installed into `PREFIX` end up in the `.conda` archive. Files in
`BUILD_PREFIX` are available during the build (the Lua interpreter itself lives
there) but are discarded afterward.

``` {.lua #prelude-globals}
-- ── Globals ───────────────────────────────────────────────────────────────────

PREFIX        = os.getenv("PREFIX")        or error("PREFIX not set")
SRC_DIR       = os.getenv("SRC_DIR")       or error("SRC_DIR not set")
BUILD_PREFIX  = os.getenv("BUILD_PREFIX")  or error("BUILD_PREFIX not set")
PKG_NAME      = os.getenv("PKG_NAME")      or error("PKG_NAME not set")
PKG_VERSION   = os.getenv("PKG_VERSION")   or error("PKG_VERSION not set")
PKG_BUILD_NUM = tonumber(os.getenv("PKG_BUILD_NUM") or "0")
```

## Internal helpers

Three local definitions handle platform detection, command execution, and path
quoting. They are not exposed to build scripts but underpin every public
function.

### `IS_WINDOWS`

Detected once from `package.config`, which always starts with the directory
separator (`\` on Windows, `/` everywhere else). Every platform-dependent
function branches on this flag.

``` {.lua #prelude-internal-helpers}
-- ── Internal helpers ──────────────────────────────────────────────────────────

--- Detect the operating system once.  `package.config` always starts with the
--- directory separator (`\` on Windows, `/` everywhere else).
local IS_WINDOWS = package.config:sub(1, 1) == "\\"
```

### `shell(cmd)`

Runs a shell command and raises a Lua error if it fails. Every file operation
in the prelude goes through `shell()`, so a failed `cp` or `mkdir` stops the
build immediately with a clear error message.

``` {.lua #prelude-internal-helpers}
local function shell(cmd)
    local ok, kind, code = os.execute(cmd)
    if not ok then
        error(string.format("Command failed (exit %d):\n  %s", code or -1, cmd), 2)
    end
end
```

### `q(path)`

Quotes a path for safe use in a shell command. On POSIX systems it wraps in
single quotes and escapes embedded single quotes. On Windows it wraps in double
quotes and normalizes forward slashes to backslashes. This is needed because
paths can contain spaces (especially on Windows, where `C:\Program Files\` is
common).

``` {.lua #prelude-internal-helpers}
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
```

## Public API

These four functions are available to any `build.lua` script.

### `path_join(...)`

Joins path segments with `/`, collapsing duplicate slashes. This is the
foundation for all path construction in the prelude.

``` {.lua #prelude-public-api}
-- ── Public API ────────────────────────────────────────────────────────────────

--- Join path segments with "/", collapsing duplicate slashes.
function path_join(...)
    local parts = {...}
    local result = table.concat(parts, "/")
    -- collapse double slashes (but keep leading "//", used on some POSIX systems)
    result = result:gsub("([^:])//+", "%1/")
    return result
end
```

### `mkdir(path)`

Creates a directory and all parent directories (like `mkdir -p`). On Windows,
`mkdir` already creates parents by default, so we suppress the "already exists"
error with `2>nul`.

``` {.lua #prelude-public-api}
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
```

### `cp(src, dst)`

Copies files. It creates the destination directory first. `src` can contain
shell globs on POSIX systems.

``` {.lua #prelude-public-api}
--- Copy `src` to `dst`.  `src` may contain shell globs.
function cp(src, dst)
    mkdir(dst)
    if IS_WINDOWS then
        shell("copy /Y " .. q(src) .. " " .. q(dst))
    else
        shell("cp -r " .. src .. " " .. q(dst))
    end
end
```

### `log(msg)`

Prints `[moonshot] msg` to stderr. Build scripts use this for progress
messages that don't interfere with stdout.

``` {.lua #prelude-public-api}
--- Print an informational message to stderr, prefixed with "[moonshot]".
function log(msg)
    io.stderr:write("[moonshot] " .. tostring(msg) .. "\n")
end
```

## Install helpers

These build on the public API to give build scripts a declarative vocabulary.

### `is_absolute(src)`

A private helper that detects absolute paths on both POSIX and Windows so
that relative source paths are expanded against `SRC_DIR`.

``` {.lua #prelude-install-helpers}
-- ── Install helpers ───────────────────────────────────────────────────────────

--- Return true when `src` is an absolute path on the current platform.
local function is_absolute(src)
    if src:sub(1, 1) == "/" then return true end
    -- Windows drive letter, e.g. "C:\" or "C:/"
    if IS_WINDOWS and src:match("^%a:[/\\]") then return true end
    return false
end
```

### `install(src, subdir)`

The base install function. Copies `src` (a path or glob) into
`PREFIX/<subdir>/`. If `src` is a relative path, it is expanded relative to
`SRC_DIR`.

``` {.lua #prelude-install-helpers}
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
```

### `install_bin(src)`

Copies executables into `PREFIX/bin/` and makes them executable on POSIX
systems. The `chmod +x` call is needed because `cp` doesn't preserve the
execute bit by default.

``` {.lua #prelude-install-helpers}
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
```

### `install_lua(src, ver)`

Copies Lua source files into `PREFIX/share/lua/<ver>/`. Defaults to version
`"5.4"`. After installation, `require("mylib")` finds the module automatically
because `share/lua/5.4/` is on the standard Lua package path. This is the
function you will use most for pure-Lua packages.

``` {.lua #prelude-install-helpers}
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

```

Finally, the prelude logs the package name, version, and prefix so you can
see what the build resolved to:

``` {.lua #prelude-done}
-- ── Done ──────────────────────────────────────────────────────────────────────

log(string.format("Building %s %s (build %d)", PKG_NAME, PKG_VERSION, PKG_BUILD_NUM))
log(string.format("PREFIX    = %s", PREFIX))
log(string.format("SRC_DIR   = %s", SRC_DIR))
```

## Summary

- The prelude sets up six globals from environment variables.
- `shell()` and `q()` handle cross-platform command execution.
- `install_lua()` is the primary function for pure-Lua packages.
- All file operations go through `shell()`, so failures are caught immediately.
