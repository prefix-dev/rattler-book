# Chapter 10: The `build` Command

<span class="newthought">We've covered</span> installing packages from existing channels. Now let's close the
loop: building a new package from source and publishing it so others can install
it.

Up to now, lumen-app has only consumed packages that someone else built.
By adding a build command, we can write our own library (`lumen`, a Lua image
toolkit that wraps ImageMagick), package it, host it on a local channel, and
install it into lumen-app with the same tool.

This is how pyproject.toml and Cargo.toml work: when you own the source, the
build configuration lives alongside the project manifest. Separate recipe files
(like conda-forge feedstocks) exist for packaging code you don't own. For
moonshot, a single `moonshot.toml` handles both.

## Design

`shot build` reads the `[build]` section from `moonshot.toml`, installs
dependencies into a temporary prefix, runs a Lua build script, packs the result
into a `.conda` archive, and indexes the output directory as a local channel.

```console
$ shot build
Building moonshine 0.3.0 (build lua_0)
  → Installing 2 build dependencies…
  → Running build script `build.lua`
  → Packing 4 files…
  → Indexing channel at /home/user/moonshine/output
✔ Built moonshine-0.3.0-lua_0.conda
  package → /home/user/moonshine/output/noarch/moonshine-0.3.0-lua_0.conda
  channel → /home/user/moonshine/output
```

You can pass `--output-dir` to control where the built `.conda` file lands
(defaults to `./output/`).

Remember `shot init`? With `--library`, it scaffolds a buildable project:

```console
$ shot init moonshine --library
✔ Created `moonshot.toml` for project "moonshine"
  Build a package with:  shot build
  Add packages with:  shot add <package>
  Install them with:  shot install
```

That creates a `moonshot.toml` with a `[build]` section already filled in:

```toml
[project]
name    = "moonshine"
version = "0.1.0"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"

[build]
script = "build.lua"
noarch = true
build_number = 0
```

The `[build]` section is what distinguishes a buildable package from a
consume-only project. Without it, `shot build` refuses to run. The `[project]`
fields `version`, `license`, and `description` are optional for consume-only
projects but `version` is required when `[build]` is present.

`[dependencies]` serves double duty: `shot install` installs them into your
environment, and `shot build` bakes them into the package as runtime
requirements.

!!! note "Dev dependencies"

    In a real-world tool you'd want a `[dev-dependencies]` section for packages
    needed during development (test runners, linters) but that shouldn't ship in
    the final package. Moonshot skips this for simplicity, but the pattern is
    the same as Cargo's or npm's dev dependencies.

The Rust struct that maps to the `[build]` section lives in `manifest.rs`
alongside `Manifest` and `ProjectMetadata` (which we defined in
[Chapter 3](ch03-init.md)). This block appends to the `manifest-structs`
block from that chapter:

``` {.rust #manifest-structs}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Defaults to "build.lua".
    #[serde(default = "default_script")]
    pub script: String,

    /// `true` for pure Lua packages (the default).
    #[serde(default = "default_noarch")]
    pub noarch: bool,

    /// Increment on rebuilds of the same version.
    #[serde(default)]
    pub build_number: u64,
}

fn default_script() -> String {
    "build.lua".to_string()
}

fn default_noarch() -> bool {
    true
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            script: default_script(),
            noarch: default_noarch(),
            build_number: 0,
        }
    }
}
```

## Concepts

### Build isolation

`shot build` creates two temporary prefixes:

- **build_prefix**: where dependencies (like the Lua interpreter) get installed.
  Tools here are available during the build but never leak into the final
  package.
- **install_prefix**: the "fake root" where the build script installs files.
  Everything in here ends up in the package.

This separation prevents build tools from accidentally becoming runtime
dependencies, making the package larger and less portable. It's the same
principle behind Debian's Build-Depends vs Depends, and it's a requirement for
reproducible builds.

### The `.conda` format

The `.conda` format (version 2) is an uncompressed ZIP containing two inner
`.tar.zst` archives: one for the info metadata and one for the payload files.
`rattler_package_streaming::write::write_conda_package` handles creating this
structure.

!!! note "Deep dive"

    For a detailed reference on what's inside a `.conda` archive, including
    `info/index.json`, `info/paths.json`, and the outer ZIP structure, see
    [Deep Dive: The conda Package Format](deep-dive-package-format.md).

### `noarch` packages

When `noarch` is true (as it is for pure-Lua packages), the package is built
once and works on all platforms, stored under the `noarch/` subdirectory. When
false, the package is platform-specific and must be built separately for each
target. Choosing `noarch` where possible reduces build and hosting costs, but
any package containing compiled code or platform-specific paths must be built
per-platform.

## Implementation

We built `Session::resolve_and_install` in [Chapter 7](ch07-install.md) for
`shot install`. Now we reuse it to install build dependencies into a temporary
prefix. That's one of the payoffs of keeping build configuration in the
manifest.

First, a few helper methods on `Manifest` that derive filenames and paths
from the metadata. These append to the `manifest-impl` block from
[Chapter 3](ch03-init.md):

``` {.rust #manifest-build-helpers}
/// The build string encoded in the package filename, e.g. `"lua_0"`.
pub fn build_string(&self) -> String {
    let build_number = self.build.as_ref().map_or(0, |b| b.build_number);
    format!("lua_{}", build_number)
}

/// The canonical filename of the output package (without directory).
///
/// e.g. `"moonshine-0.3.0-lua_0.conda"`
pub fn package_filename(&self) -> miette::Result<String> {
    let version = self.project.version.as_deref().ok_or_else(|| {
        miette::miette!("No `version` in [project]. A version is required to build a package.")
    })?;
    Ok(format!(
        "{}-{}-{}.conda",
        self.project.name,
        version,
        self.build_string()
    ))
}

/// The subdirectory where the package should live in a channel.
///
/// Noarch packages go in `noarch/`; platform-specific packages go in
/// e.g. `linux-64/`.
pub fn subdir(&self) -> &'static str {
    match &self.build {
        Some(b) if b.noarch => "noarch",
        _ => rattler_conda_types::Platform::current().as_str(),
    }
}
```

### The `BuildBackend` trait

Before looking at the build command itself, we introduce a `BuildBackend`
trait that encapsulates how build scripts are executed. This lives in
`src/build_backend.rs`:

``` {.rust file=src/build_backend.rs}
<<build-backend-imports>>

<<build-context-struct>>

<<build-backend-trait>>

<<lua-backend-const>>

<<lua-backend-struct>>

<<lua-backend-impl>>

<<lua-find-lua>>

<<lua-run-build-script>>
```

#### Imports

``` {.rust #build-backend-imports}
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};

use crate::manifest::Manifest;
```

#### The `BuildContext` struct

``` {.rust #build-context-struct}
/// Context passed to a [`BuildBackend`] when executing a build.
pub struct BuildContext<'a> {
    pub manifest: &'a Manifest,
    pub src_dir: PathBuf,
    pub install_prefix: PathBuf,
    pub build_prefix: PathBuf,
}
```

#### The trait

``` {.rust #build-backend-trait}
/// A pluggable build backend.
///
/// Implement this trait to add support for new build-script languages
/// beyond Lua.
#[allow(dead_code)]
pub trait BuildBackend {
    /// Human-readable name of this backend, for log messages.
    fn name(&self) -> &str;

    /// Run the build script, installing files into `ctx.install_prefix`.
    fn run_build(
        &self,
        ctx: &BuildContext<'_>,
    ) -> impl std::future::Future<Output = miette::Result<()>> + Send;
}
```

The trait is generic over the build-script language. Today we only have Lua,
but the design allows adding Python, shell, or other backends later.

#### The Lua backend

``` {.rust #lua-backend-const}
const BUILD_PRELUDE: &str = include_str!("build_prelude.lua");
```

``` {.rust #lua-backend-struct}
/// The default build backend — runs a Lua build script.
pub struct LuaBuildBackend;
```

``` {.rust #lua-backend-impl}
impl BuildBackend for LuaBuildBackend {
    fn name(&self) -> &str {
        "lua"
    }

    async fn run_build(&self, ctx: &BuildContext<'_>) -> miette::Result<()> {
        let build_config = ctx
            .manifest
            .build
            .as_ref()
            .expect("[build] section validated in execute()");

        let script_path = ctx.src_dir.join(&build_config.script);
        if !script_path.exists() {
            miette::bail!(
                "Build script `{}` not found (expected at `{}`)",
                build_config.script,
                script_path.display()
            );
        }

        let lua_bin = find_lua(&ctx.build_prefix)?;

        println!(
            "  {} Running build script `{}`",
            console::style("→").blue(),
            build_config.script
        );

        run_build_script(
            &lua_bin,
            &script_path,
            &ctx.install_prefix,
            &ctx.src_dir,
            &ctx.build_prefix,
            ctx.manifest,
        )
        .await
    }
}
```

#### Finding the Lua interpreter

``` {.rust #lua-find-lua}
fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
    let bin_dirs: &[&str] = if cfg!(windows) {
        &["Library/bin", "bin"]
    } else {
        &["bin"]
    };
    let exe_ext = if cfg!(windows) { ".exe" } else { "" };

    for bin_dir in bin_dirs {
        let lua = prefix.join(bin_dir).join(format!("lua{exe_ext}"));
        if lua.exists() {
            return Ok(lua);
        }
        for minor in (1u8..=4u8).rev() {
            let versioned = prefix.join(bin_dir).join(format!("lua5.{minor}{exe_ext}"));
            if versioned.exists() {
                return Ok(versioned);
            }
        }
    }

    let searched: Vec<_> = bin_dirs
        .iter()
        .map(|d| prefix.join(d).display().to_string())
        .collect();
    miette::bail!(
        "No Lua interpreter found in `{}`. \
         Add `lua` to [dependencies] in moonshot.toml.",
        searched.join("`, `")
    )
}
```

#### Running the build script

We locate the Lua interpreter in the build prefix and run the build script
through a wrapper that loads the prelude first. We use a wrapper file rather
than `-e '...'` so that error messages show correct line numbers.

The build script can use any tool installed in `build_prefix/bin` because we
prepend it to `PATH`.

``` {.rust #lua-run-build-script}
async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    let wrapper_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating wrapper temp dir")?;

    let prelude_path = wrapper_dir.path().join("prelude.lua");
    std::fs::write(&prelude_path, BUILD_PRELUDE)
        .into_diagnostic()
        .context("writing build prelude")?;

    let wrapper_src = format!(
        "dofile({prelude:?})\ndofile({script:?})\n",
        prelude = prelude_path.to_string_lossy(),
        script = script.to_string_lossy(),
    );
    let wrapper_path = wrapper_dir.path().join("wrapper.lua");
    std::fs::write(&wrapper_path, &wrapper_src)
        .into_diagnostic()
        .context("writing build wrapper")?;

    let original_path = std::env::var("PATH").unwrap_or_default();
    let path_sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = if cfg!(windows) {
        format!(
            "{}{path_sep}{}{path_sep}{original_path}",
            build_prefix.join("Library").join("bin").display(),
            build_prefix.join("bin").display(),
        )
    } else {
        format!(
            "{}{path_sep}{original_path}",
            build_prefix.join("bin").display(),
        )
    };

    let build_config = manifest
        .build
        .as_ref()
        .expect("[build] section validated in execute()");

    let status = tokio::process::Command::new(lua_bin)
        .arg(&wrapper_path)
        .env("PREFIX", install_prefix)
        .env("SRC_DIR", src_dir)
        .env("BUILD_PREFIX", build_prefix)
        .env("PKG_NAME", &manifest.project.name)
        .env(
            "PKG_VERSION",
            manifest.project.version.as_deref().unwrap_or("0.0.0"),
        )
        .env("PKG_BUILD_NUM", build_config.build_number.to_string())
        .env("PATH", &new_path)
        .status()
        .await
        .into_diagnostic()
        .context("launching Lua interpreter")?;

    if !status.success() {
        miette::bail!(
            "Build script exited with status {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
```

### The build command

Here is the full file skeleton for `src/commands/build.rs`, with each section
defined as we encounter it:

``` {.rust file=src/commands/build.rs}
<<build-imports>>

<<build-args>>

<<build-execute>>

<<build-write-metadata>>

<<build-collect-paths>>

<<build-sha256>>

<<build-pack-conda>>
```

#### Imports

``` {.rust #build-imports}
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_conda_types::package::{IndexJson, PackageFile, PathType, PathsEntry, PathsJson};
use rattler_conda_types::{NoArchType, PackageName, VersionWithSource};
use rattler_index::{index_fs, IndexFsConfig};
use rattler_package_streaming::write::write_conda_package;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::build_backend::{BuildBackend, BuildContext, LuaBuildBackend};
use crate::manifest::{Manifest, MANIFEST_FILENAME};
use crate::project::Project;
use crate::session::Session;
```

#### CLI arguments

``` {.rust #build-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Directory where the built `.conda` file is written.
    ///
    /// Defaults to `./output/`.
    #[clap(long, default_value = "output")]
    pub output_dir: PathBuf,
}
```

#### The `execute` function

The build has five stages: discover the project, set up working directories,
install dependencies, run the build script via the backend, and pack the result.

``` {.rust #build-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let session = Session::new(project)?;
    let manifest = &session.project.manifest;
    let cwd = &session.project.root;

    let _build_config = manifest.build.as_ref().ok_or_else(|| {
        miette::miette!(
            "No [build] section in `{MANIFEST_FILENAME}`. \
             Add one to make this project buildable, or run \
             `shot init --library` to start a new library project."
        )
    })?;

    let version = manifest.project.version.as_deref().ok_or_else(|| {
        miette::miette!(
            "No `version` in [project]. \
             A version is required to build a package."
        )
    })?;

    println!(
        "Building {} {} (build {})",
        console::style(&manifest.project.name).cyan(),
        version,
        manifest.build_string(),
    );

    let work_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating temporary build directory")?;

    let build_prefix = work_dir.path().join("build_prefix");
    let install_prefix = work_dir.path().join("install_prefix");
    std::fs::create_dir_all(&build_prefix)
        .into_diagnostic()
        .context("creating build_prefix")?;
    std::fs::create_dir_all(&install_prefix)
        .into_diagnostic()
        .context("creating install_prefix")?;

    let src_dir = std::path::absolute(cwd)
        .into_diagnostic()
        .context("resolving SRC_DIR")?;

    if !manifest.dependencies.is_empty() {
        println!(
            "  {} Installing {} build dependencies…",
            console::style("→").blue(),
            manifest.dependencies.len()
        );
        session.resolve_and_install(build_prefix.clone()).await?;
    }

    let backend = LuaBuildBackend;
    let ctx = BuildContext {
        manifest,
        src_dir: src_dir.clone(),
        install_prefix: install_prefix.clone(),
        build_prefix: build_prefix.clone(),
    };
    backend.run_build(&ctx).await?;

    <<pack-and-index>>
}
```

The execute function now uses `Session` and delegates build-script execution
to the `LuaBuildBackend` via the `BuildBackend` trait. The `BuildContext`
bundles all the paths and metadata the backend needs.

##### Packing and indexing

We write package metadata, pack the `.conda` archive, and index the output
channel so other tools can use it.

``` {.rust #pack-and-index}
write_package_metadata(&install_prefix, manifest).context("writing package metadata")?;

let output_dir = std::path::absolute(&args.output_dir)
    .into_diagnostic()
    .context("resolving output directory")?;

let subdir_dir = output_dir.join(manifest.subdir());
std::fs::create_dir_all(&subdir_dir)
    .into_diagnostic()
    .context("creating output subdir")?;

let filename = manifest.package_filename()?;
let output_path = subdir_dir.join(&filename);

pack_conda(&install_prefix, &output_path, manifest)?;
```

``` {.rust #pack-and-index}
println!(
    "  {} Indexing channel at {}",
    console::style("→").blue(),
    output_dir.display()
);
index_fs(IndexFsConfig {
    channel: output_dir.clone(),
    target_platform: None, // discover all subdirs automatically
    repodata_patch: None,
    write_zst: true,
    write_shards: true,
    force: false, // incremental (only index new packages)
    max_parallel: 4,
    multi_progress: None,
})
.await
.map_err(|e| miette::miette!("{e:#}"))
.context("indexing output channel")?;

println!(
    "{} Built {}",
    console::style("✔").green(),
    console::style(&filename).cyan()
);
println!("  package → {}", output_path.display());
println!("  channel → {}", output_dir.display());

Ok(())
```

### The build script prelude

Writing a build script that manually uses `os.execute("cp ...")` works but is
tedious. So we embed a Lua prelude that provides helper functions. The prelude
runs before every build script, setting up globals (`PREFIX`, `SRC_DIR`,
`BUILD_PREFIX`, `PKG_NAME`, `PKG_VERSION`, `PKG_BUILD_NUM`) and providing
file-system helpers (`mkdir`, `cp`, `install_lua`, `install_bin`, and more).

For a detailed walkthrough of each function, see
[Deep Dive: The Build Script API](deep-dive-build-script-api.md).

Here is the complete `src/build_prelude.lua`:

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
--
-- You do NOT need to require() this file; everything below is already
-- in scope when your build script runs.
--
-- Available globals
-- -----------------
--   PREFIX        Where the package should be installed
--   SRC_DIR       Root of your source tree
--   BUILD_PREFIX  Where build-time dependencies live (e.g. lua itself)
--   PKG_NAME      Package name from moonshot.toml
--   PKG_VERSION   Package version from moonshot.toml
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
```

``` {.lua #prelude-globals}
-- ── Globals ───────────────────────────────────────────────────────────────────

PREFIX        = os.getenv("PREFIX")        or error("PREFIX not set")
SRC_DIR       = os.getenv("SRC_DIR")       or error("SRC_DIR not set")
BUILD_PREFIX  = os.getenv("BUILD_PREFIX")  or error("BUILD_PREFIX not set")
PKG_NAME      = os.getenv("PKG_NAME")      or error("PKG_NAME not set")
PKG_VERSION   = os.getenv("PKG_VERSION")   or error("PKG_VERSION not set")
PKG_BUILD_NUM = tonumber(os.getenv("PKG_BUILD_NUM") or "0")
```

``` {.lua #prelude-internal-helpers}
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
```

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

--- Move `src` to `dst`.
function mv(src, dst)
    if IS_WINDOWS then
        shell("move /Y " .. q(src) .. " " .. q(dst))
    else
        shell("mv " .. q(src) .. " " .. q(dst))
    end
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
```

``` {.lua #prelude-install-helpers}
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
---   install_share("docs/", "lumen")
function install_share(src, name)
    install(src, path_join("share", name))
end
```

``` {.lua #prelude-done}
-- ── Done ──────────────────────────────────────────────────────────────────────

log(string.format("Building %s %s (build %d)", PKG_NAME, PKG_VERSION, PKG_BUILD_NUM))
log(string.format("PREFIX    = %s", PREFIX))
log(string.format("SRC_DIR   = %s", SRC_DIR))
```

### Packaging

Once the build script finishes, we turn the install prefix into a `.conda`
archive. This involves writing metadata files, collecting file hashes, packing
the archive, and indexing the output channel.

#### Writing package metadata

Every conda package contains an `info/` directory with metadata. We need two
files: `index.json` (which the solver reads to understand the package) and
`paths.json` (which lists every file with its checksum).

``` {.rust #build-write-metadata}
fn write_package_metadata(install_prefix: &Path, manifest: &Manifest) -> miette::Result<()> {
    <<create-index-json>>
    <<write-meta-files>>
}
```

``` {.rust #create-index-json}
let info_dir = install_prefix.join("info");
std::fs::create_dir_all(&info_dir)
    .into_diagnostic()
    .context("creating info/ directory")?;

let build_config = manifest
    .build
    .as_ref()
    .expect("[build] section validated in execute()");

let noarch = if build_config.noarch {
    NoArchType::generic()
} else {
    NoArchType::default()
};

let subdir = if build_config.noarch {
    Some("noarch".to_string())
} else {
    Some(rattler_conda_types::Platform::current().to_string())
};

let version_str = manifest.project.version.as_deref().unwrap_or("0.0.0");

let index = IndexJson {
    name: PackageName::from_str(&manifest.project.name)
        .into_diagnostic()
        .with_context(|| format!("invalid package name `{}`", manifest.project.name))?,
    version: VersionWithSource::from_str(version_str)
        .into_diagnostic()
        .with_context(|| format!("invalid version `{}`", version_str))?,
    build: manifest.build_string(),
    build_number: build_config.build_number,
    subdir,
    arch: None,
    platform: None,
    noarch,
    depends: manifest.dependency_strings(),
    constrains: vec![],
    experimental_extra_depends: Default::default(),
    features: None,
    license: manifest.project.license.clone(),
    license_family: None,
    purls: None,
    python_site_packages_path: None,
    track_features: vec![],
    timestamp: Some(
        rattler_conda_types::utils::TimestampMs::from_datetime_millis(chrono::Utc::now()),
    ),
};
```

``` {.rust #write-meta-files}
let index_path = install_prefix.join(IndexJson::package_path());
let index_json = serde_json::to_string_pretty(&index)
    .into_diagnostic()
    .context("serializing index.json")?;
std::fs::write(&index_path, index_json)
    .into_diagnostic()
    .context("writing info/index.json")?;

let paths = collect_paths_json(install_prefix).context("building paths.json")?;

let paths_path = install_prefix.join(PathsJson::package_path());
let paths_json = serde_json::to_string_pretty(&paths)
    .into_diagnostic()
    .context("serializing paths.json")?;
std::fs::write(&paths_path, paths_json)
    .into_diagnostic()
    .context("writing info/paths.json")?;

Ok(())
```

#### Collecting paths and hashing

We walk the install prefix, hash every file with SHA-256, and record each path
in a `PathsJson` manifest:

``` {.rust #build-collect-paths}
fn collect_paths_json(prefix: &Path) -> miette::Result<PathsJson> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(prefix).into_iter().filter_map(|e| e.ok()) {
        let meta = entry.metadata().into_diagnostic()?;
        if !meta.is_file() {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(prefix)
            .into_diagnostic()
            .context("stripping prefix from path")?
            .to_path_buf();

        // Compute SHA-256 and size for integrity checking.
        let (sha256, size) = sha256_and_size(abs_path)?;

        entries.push(PathsEntry {
            relative_path: rel_path,
            no_link: false,
            path_type: PathType::HardLink,
            prefix_placeholder: None,
            sha256: Some(sha256),
            size_in_bytes: Some(size),
        });
    }

    Ok(PathsJson {
        paths: entries,
        paths_version: 1,
    })
}
```

``` {.rust #build-sha256}
fn sha256_and_size(path: &Path) -> miette::Result<(rattler_digest::Sha256Hash, u64)> {
    use std::io::Read;
    let file = File::open(path)
        .into_diagnostic()
        .with_context(|| format!("opening `{}`", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf).into_diagnostic()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hasher.finalize(), size))
}
```

#### Packing into `.conda`

We pass the install prefix and its file list to `write_conda_package`, which
separates `info/` files from payload files, compresses each group into a
`.tar.zst`, and wraps both into an uncompressed ZIP:

``` {.rust #build-pack-conda}
fn pack_conda(
    install_prefix: &Path,
    output_path: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    // Collect all files relative to the install prefix.
    let files: Vec<PathBuf> = WalkDir::new(install_prefix)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    if files.is_empty() {
        miette::bail!(
            "The build script did not install any files into PREFIX (`{}`). \
             Make sure your build.lua copies files to `os.getenv(\"PREFIX\")`.",
            install_prefix.display()
        );
    }

    println!(
        "  {} Packing {} files…",
        console::style("→").blue(),
        files.len()
    );

    let writer = BufWriter::new(
        File::create(output_path)
            .into_diagnostic()
            .with_context(|| format!("creating output file `{}`", output_path.display()))?,
    );

    let out_name = format!(
        "{}-{}-{}",
        manifest.project.name,
        manifest.project.version.as_deref().unwrap_or("0.0.0"),
        manifest.build_string()
    );

    let now = chrono::Utc::now();
    write_conda_package(
        writer,
        install_prefix,
        &files,
        CompressionLevel::Default,
        None, // use all available CPU threads for zstd
        &out_name,
        Some(&now),
        None, // no progress bar (already shown by our spinner)
    )
    .into_diagnostic()
    .context("writing .conda archive")?;

    Ok(())
}
```

#### Indexing the channel

After packing, the output directory isn't yet a valid conda channel. It has
packages but no `repodata.json`. The `index_fs` call inside `execute` scans
the directory, reads every `.conda` file's `info/index.json`, and writes:

- `output/noarch/repodata.json`, the plain JSON catalog
- `output/noarch/repodata.json.zst`, a compressed version
- `output/noarch/repodata_shards.msgpack.zst`, the sharded format

Once indexed, the output directory can be used as a channel:

```toml
# Another project's moonshot.toml
[project]
channels = ["./output", "conda-forge"]

[dependencies]
moonshine = ">=0.3"
```

!!! note "Beyond local indexing"

    For a fully-featured package manager, local indexing is only the first step.
    You would also need a way to push packages to a remote server, sign them so
    consumers can verify authenticity, and define a trust model (who is allowed
    to publish, and how do you revoke a compromised key). These are substantial
    features that we skip in moonshot, but they are the difference between a local
    build tool and a real distribution system.

## Try it

Let's build a package and install it in lumen-app to see the full loop in
action. We'll create `lumen`, a Lua image toolkit that wraps ImageMagick.

First, create a library project:

```console
$ mkdir lumen && cd lumen
$ shot init lumen --library
✔ Created `moonshot.toml` for project "lumen"
```

Edit `moonshot.toml` to add imagemagick as a dependency. Anyone who installs
lumen will get imagemagick automatically:

```toml
[project]
name = "lumen"
version = "0.1.0"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
imagemagick = "*"

[build]
script = "build.lua"
noarch = true
```

Write a Lua module that wraps the `magick` command-line tool. ImageMagick is
a C library with dozens of native dependencies (libpng, libtiff, zlib, and
more). conda-forge provides all of them pre-built; our Lua code just shells
out to `magick`:

```lua
-- lumen.lua
local M = {}

function M.thumbnail(input, size)
    size = size or 128
    local output = input:gsub("(%..+)$", "_thumb%1")
    local cmd = string.format("magick %s -thumbnail %dx%d %s",
        input, size, size, output)
    local ok = os.execute(cmd)
    if not ok then error("magick failed -- is imagemagick installed?") end
    return output
end

function M.grayscale(input)
    local output = input:gsub("(%..+)$", "_gray%1")
    local cmd = string.format("magick %s -colorspace Gray %s", input, output)
    local ok = os.execute(cmd)
    if not ok then error("magick failed -- is imagemagick installed?") end
    return output
end

return M
```

And a build script that installs it:

```lua
-- build.lua
install_lua("lumen.lua")
```

Build the package:

```console
$ shot build
Building lumen 0.1.0 (build lua_0)
  → Installing 2 build dependencies…
  → Running build script `build.lua`
  → Packing 3 files…
  → Indexing channel at /home/user/lumen/output
✔ Built lumen-0.1.0-lua_0.conda
  package → /home/user/lumen/output/noarch/lumen-0.1.0-lua_0.conda
  channel → /home/user/lumen/output
```

Now go back to lumen-app (the project we created in [Chapter 3](ch03-init.md))
and add lumen as a dependency using the local channel:

```console
$ cd ../lumen-app
```

Edit `moonshot.toml`:

```toml
[project]
name = "lumen-app"
channels = ["../lumen/output", "conda-forge"]

[dependencies]
lua = ">=5.4"
lumen = "*"
```

Install and run:

```console
$ shot install
$ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
```

That creates `photo_thumb.jpg`, a 128-pixel thumbnail. ImageMagick and all its
native dependencies were installed by the package manager because lumen declared
them in its manifest.

That's the full loop. The `.conda` file you built is the same format that
conda-forge uses to distribute tens of thousands of packages.

<!-- TODO: Exercises
- Inspect the output .conda file with `unzip -l output/noarch/lumen-0.1.0-lua_0.conda`. What files are inside?
- Extract info-*.tar.zst and look at info/index.json. What dependencies does it list?
- Try building without imagemagick in [dependencies]. Does the build succeed? Does installation of lumen in another project still work?
- Create a second library that depends on lumen. Can you chain local channels?
-->

## Summary

- The `[build]` section in `moonshot.toml` turns a project into a buildable
  package.
- Dependencies are installed into a temporary build prefix, keeping build tools
  separate from the final package.
- `paths.json` lists every file with its SHA-256 hash.
- `write_conda_package` produces the `.conda` archive format.
- `rattler_index` turns the output directory into a valid conda channel.

With `shot build` working, our package manager is feature-complete! In Part II
we'll look deeper into the mechanisms we've been using.
