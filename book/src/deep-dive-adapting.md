# Deep Dive: Adapting to Your Language

<span class="newthought">Moonshot builds Lua packages,</span> but the rattler crates underneath have no opinion about programming languages. If you want to build a package manager for Ruby, Zig, Haskell, or anything else, most of moonshot's code transfers directly. This chapter maps out what to keep and what to change.

## What works for any language

Chapters 1 through 9 are language-agnostic. Here is a quick inventory:

- **Manifest and init** (ch3): the `[project]` and `[dependencies]` format works for any language. You would swap the default dependency (from `lua >=5.4` to your language runtime).
- **Search and add** (ch4, ch5): repodata queries and manifest editing have nothing language-specific in them.
- **Lock and solve** (ch6): the solver, lock file format, and `Session` abstraction work unchanged.
- **Install** (ch7): the installer handles any conda package, regardless of what is inside.
- **Shell-hook and run** (ch8, ch9): environment activation and subprocess launching are generic.

All of these pieces can move into a new project as-is.

## What changes: the build command

Chapter 10 is where Lua-specific decisions concentrate. For a different language, here is what you would replace or extend.

### 1. The `noarch` default

Moonshot defaults to `noarch: generic` because Lua scripts are platform-independent. For compiled languages, you want platform-specific packages instead. The `subdir()` helper already has both branches, so flipping this default is a one-line change.

### 2. The build backend

Moonshot ships a `LuaBuildBackend` that embeds a small Lua prelude. You would implement a new backend for your language, one that calls `cargo build`, `go build`, `make install`, or whatever your ecosystem uses. The `BuildBackend` trait exists for exactly this purpose.

### 3. Dependency sections

Moonshot uses a single `[dependencies]` table for both install-time and build-time dependencies. That works fine for a scripting language where there is no compilation step, but compiled languages need more structure:

- `[host-dependencies]`: libraries to link against (e.g., `openssl`, `zlib`)
- `[build-dependencies]`: the compiler itself, cmake, pkg-config

Rather than implementing this split yourself, consider using [rattler-build](https://github.com/prefix-dev/rattler-build) as a library. It handles the host/build/run separation, build string hashing, `run_exports`, and compiler activation.

### 4. The build string

Moonshot hardcodes `"lua_0"` as the build string. For compiled packages, the build string should encode a hash of the build inputs:

- compiler version
- host dependency versions
- variant configuration

This is what lets the solver distinguish two builds of the same package compiled against different versions of OpenSSL. rattler-build computes this hash automatically.

### 5. Interpreter lookup

Moonshot's `find_lua` function searches for the Lua binary in the environment. For a compiled language there is no interpreter to find. Instead, compiler packages ship `activate.d/` scripts that set environment variables like `CC`, `CXX`, and `CFLAGS`. The activation mechanism from Chapter 8 already handles running these scripts.

### 6. File modes in prefix replacement

Moonshot sets `FileMode::Text` when recording prefix placeholders. For packages containing compiled binaries or shared libraries, you would detect the file type and use `FileMode::Binary` for executables and `.so`/`.dylib` files. This matters because text and binary files use different placeholder replacement strategies.

## Custom virtual packages

Your language runtime can be modeled as a virtual package. The [deep dive on virtual packages](deep-dive-virtual-packages.md) shows how to construct a `GenericVirtualPackage` by hand:

```rust
GenericVirtualPackage {
    name: "__ruby".parse().unwrap(),
    version: "3.3".parse().unwrap(),
    build_string: "0".to_string(),
}
```

This lets package authors write `__ruby >=3.2` in their dependencies, and the solver handles it like any other version constraint.

## What moonshot deliberately skips

Moonshot is a teaching tool. A production package manager would need several features it leaves out. For each one, there is an existing crate or tool you can reach for:

- **`run_exports`**: a build dependency automatically adds runtime dependencies to the output package. See the [run exports deep dive](deep-dive-run-exports.md) and rattler-build.
- **Cross-platform virtual packages**: moonshot detects from the host only, so solving for a different platform gives wrong results. The exercise in Chapter 6 addresses this. Pixi provides per-platform defaults.
- **Multi-output recipes**: a single source producing multiple packages (e.g., `libfoo` and `libfoo-dev`). Handled by rattler-build.
- **Source fetching**: downloading and extracting source tarballs or git repos before building. Handled by rattler-build.
- **Test running**: conda packages can carry test scripts in `info/test/`. Moonshot does not run them.

The rattler crates handle the hard parts: solving, installing, networking, and activation. Your job is the manifest format, the build system, and the language-specific glue. Moonshot shows one way to wire these pieces together.
