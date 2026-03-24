# Deep Dive: The Build Script API

When `shot build` runs your `build.lua`, it first loads a prelude that sets up
globals and helper functions. This chapter walks through each one. The code
itself lives in [Chapter 10](ch10-build.md); here we explain the design
decisions.

## Globals

The Rust side sets environment variables before launching the Lua interpreter.
The prelude reads them once and fails fast if any are missing:

| Global | Source | Meaning |
|---|---|---|
| `PREFIX` | `--install-prefix` temp dir | Where the package should install files |
| `SRC_DIR` | current working directory | Root of your source tree |
| `BUILD_PREFIX` | `--build-prefix` temp dir | Where build dependencies live |
| `PKG_NAME` | `moonshot.toml [project].name` | Package name |
| `PKG_VERSION` | `moonshot.toml [project].version` | Package version |
| `PKG_BUILD_NUM` | `moonshot.toml [build].build_number` | Build number (integer) |

`PREFIX` and `BUILD_PREFIX` are temporary directories created by `shot build`.
Files installed into `PREFIX` end up in the `.conda` archive. Files in
`BUILD_PREFIX` are available during the build (the Lua interpreter itself lives
there) but are discarded afterward.

## Internal helpers

### `shell(cmd)`

Runs a shell command and raises a Lua error if it fails. Every file operation
in the prelude goes through `shell()`, so a failed `cp` or `mkdir` stops the
build immediately with a clear error message.

### `q(path)`

Quotes a path for safe use in a shell command. On POSIX systems it wraps in
single quotes and escapes embedded single quotes. On Windows it wraps in double
quotes and normalizes forward slashes to backslashes.

This is needed because paths can contain spaces (especially on Windows, where
`C:\Program Files\` is common).

### `IS_WINDOWS`

Detected once from `package.config`, which always starts with the directory
separator (`\` on Windows, `/` everywhere else). Every platform-dependent
function branches on this flag.

## Public API

### `path_join(...)`

Joins path segments with `/`, collapsing duplicate slashes. This is the
foundation for all path construction in the prelude.

### `mkdir(path)`

Creates a directory and all parent directories (like `mkdir -p`). On Windows,
`mkdir` already creates parents by default, so we suppress the "already exists"
error with `2>nul`.

### `cp(src, dst)`, `mv(src, dst)`

Copy and move files. `cp` creates the destination directory first. `src` can
contain shell globs on POSIX systems.

### `exists(path)`, `is_file(path)`

Portable file-existence checks using `io.open()`. `exists` returns true for
any path that can be opened for reading. `is_file` opens in binary mode and
returns true only for regular files.

### `log(msg)`

Prints `[moonshot] msg` to stderr. Build scripts use this for progress
messages that don't interfere with stdout.

## Install helpers

These build on the public API to give build scripts a declarative vocabulary.

### `install(src, subdir)`

The base install function. Copies `src` (a path or glob) into
`PREFIX/<subdir>/`. If `src` is a relative path, it's expanded relative to
`SRC_DIR`.

### `install_lua(src, ver)`

Copies Lua source files into `PREFIX/share/lua/<ver>/`. Defaults to version
`"5.4"`. After installation, `require("mylib")` finds the module
automatically because `share/lua/5.4/` is on the standard Lua package path.

This is the function you'll use most for pure-Lua packages.

### `install_bin(src)`

Copies executables into `PREFIX/bin/` and makes them executable on POSIX
systems. The `chmod +x` call is needed because `cp` doesn't preserve the
execute bit by default.

### `install_lib(src)`

Copies files into `PREFIX/lib/`. Intended for shared libraries (`.so`,
`.dylib`, `.dll`).

### `install_share(src, name)`

Copies files into `PREFIX/share/<name>/`. Useful for data files,
documentation, or anything that doesn't fit the other categories.

## Summary

- The prelude sets up six globals from environment variables.
- `shell()` and `q()` handle cross-platform command execution.
- `install_lua()` is the primary function for pure-Lua packages.
- All file operations go through `shell()`, so failures are caught immediately.
