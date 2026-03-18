# Chapter 9: The `build` Command

We've covered installing packages from existing channels.  Now let's close the
loop: building a new package from source and publishing it so others can install
it.

Moving from consumer to producer makes moonshot self-sufficient for the Lua ecosystem. Up to now, moonshot only consumed packages that someone else built and uploaded. A package manager that can only consume depends on external tooling (like [conda-build] or [rattler-build]) to create new packages. By adding a build command, you can write a library, package it, host it on a local channel, and install it with the same tool.

## Design

`shot build` reads a `recipe.toml`, installs build-time dependencies into a
temporary prefix, runs a Lua build script, packs the result into a `.conda`
archive, and indexes the output directory as a local channel.

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

The command accepts `--recipe` (path to recipe.toml, defaults to `./recipe.toml`)
and `--output-dir` (defaults to `./output/`).

## Configuration: `recipe.toml`

```toml
# recipe.toml
[package]
name    = "moonshine"
version = "0.3.0"
license = "MIT"

[source]
path = "."

[channels]
list = ["conda-forge"]

[requirements]
run   = ["lua >=5.4"]
build = ["lua >=5.4"]

[build]
script = "build.lua"
noarch = true
```

The recipe lives in your project directory alongside your source code.

The Rust struct mirrors this structure. Here is the full `src/recipe.rs`
assembled from the pieces we walk through below:

``` {.rust file=src/recipe.rs}
<<recipe-imports>>

<<recipe-filename-const>>

<<recipe-structs>>

<<recipe-impl>>
```

#### Imports

``` {.rust #recipe-imports}
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
```

#### The recipe filename

``` {.rust #recipe-filename-const}
/// File name we look for in the current directory.
pub const RECIPE_FILENAME: &str = "recipe.toml";
```

A single constant for the filename keeps it consistent across all commands,
the same approach used for `MANIFEST_FILENAME` in `manifest.rs`.

#### Structs

``` {.rust #recipe-structs}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub package: PackageMeta,

    #[serde(default)]
    pub source: SourceSpec,

    #[serde(default)]
    pub channels: ChannelSpec,

    #[serde(default)]
    pub requirements: Requirements,

    #[serde(default)]
    pub build: BuildConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    /// Lowercase, hyphens allowed.
    pub name: String,

    /// Semantic version string, e.g. `"1.2.3"`.
    pub version: String,

    /// Increment on rebuilds of the same version.
    #[serde(default)]
    pub build_number: u64,

    /// SPDX license identifier, e.g. `"MIT"`.
    pub license: Option<String>,

    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// Absolute or relative to the recipe. Defaults to `"."`.
    #[serde(default = "dot")]
    pub path: String,
}

fn dot() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSpec {
    /// Channel list, in priority order.  Defaults to `["conda-forge"]`.
    #[serde(default = "default_channels")]
    pub list: Vec<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}

impl Default for ChannelSpec {
    fn default() -> Self {
        Self {
            list: default_channels(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Requirements {
    #[serde(default)]
    pub build: Vec<String>,

    #[serde(default)]
    pub run: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Defaults to `"build.lua"`.
    #[serde(default = "default_script")]
    pub script: String,

    /// `true` for pure Lua packages (the default).
    #[serde(default = "default_noarch")]
    pub noarch: bool,
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
        }
    }
}
```

Each struct maps to a TOML section: `Recipe` is the top level, `PackageMeta`
is `[package]`, `SourceSpec` is `[source]`, and so on. A few points about the
schema:

- Package names must be lowercase with hyphens allowed (no spaces, no uppercase).
- `build_number` is a monotonically-increasing integer. Increment it when you
  rebuild the same version with different settings (patched deps, different
  compiler flags). The solver treats a higher build number as "more recent."
- Source paths can be absolute or relative to the recipe file. The default
  `"."` means the directory containing `recipe.toml`.

The `#[serde(default)]` annotations make the `[source]`, `[channels]`,
`[requirements]`, and `[build]` sections optional.

#### Methods

The methods on `Recipe` handle loading from disk and computing filenames for
the output package:

``` {.rust #recipe-impl}
impl Recipe {
    /// Read a `recipe.toml` from a directory.
    #[allow(dead_code)]
    pub fn find_in_dir(dir: &Path) -> miette::Result<(PathBuf, Self)> {
        let path = dir.join(RECIPE_FILENAME);
        if !path.exists() {
            miette::bail!(
                "No `{RECIPE_FILENAME}` found in `{}`. \
                 Create one to build a package.",
                dir.display()
            );
        }
        let recipe = Self::from_path(&path)?;
        Ok((path, recipe))
    }

    /// Parse a `recipe.toml` at the given path.
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .with_context(|| format!("reading recipe at `{}`", path.display()))?;

        toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing recipe at `{}`", path.display()))
    }

    /// The build string encoded in the package filename, e.g. `"lua_0"`.
    pub fn build_string(&self) -> String {
        format!("lua_{}", self.package.build_number)
    }

    /// The canonical filename of the output package (without directory).
    ///
    /// e.g. `"moonshine-0.3.0-lua_0.conda"`
    pub fn package_filename(&self) -> String {
        format!(
            "{}-{}-{}.conda",
            self.package.name,
            self.package.version,
            self.build_string()
        )
    }

    /// The subdirectory where the package should live in a channel.
    ///
    /// Noarch packages go in `noarch/`; platform-specific packages go in
    /// e.g. `linux-64/`.
    #[allow(dead_code)]
    pub fn subdir(&self) -> &'static str {
        if self.build.noarch {
            "noarch"
        } else {
            rattler_conda_types::Platform::current().as_str()
        }
    }
}
```

## Concepts

### Build isolation

The two-prefix design is build isolation. Tools in `build_prefix`
(compilers, interpreters, build utilities) are available during the build
but never leak into the final package. Without this separation, a build tool
could accidentally end up as a runtime dependency, making the package larger
and less portable. This is the same principle behind Debian's Build-Depends
vs Depends, and it is a key requirement for reproducible builds.

### The `.conda` format

The `.conda` format (version 2) is an uncompressed ZIP containing two inner
`.tar.zst` archives: one for the info metadata and one for the payload files.
`rattler_package_streaming::write::write_conda_package` handles creating this
structure.

!!! note "Deep dive"

    For a detailed reference on what's inside a `.conda` archive, including
    `info/index.json`, `info/paths.json`, and the outer ZIP structure, see
    [Deep Dive: The conda Package Format](deep-dive-package-format.md).

### noarch packages

When `noarch` is true (as it is for pure-Lua packages), the package is built
once and works on all platforms, stored under the `noarch/` subdirectory. When
false, the package is platform-specific and must be built separately for each
target. Choosing `noarch` where possible reduces build and hosting costs, but
any package containing compiled code or platform-specific paths must be built
per-platform.

## Implementation

### The build script prelude

Writing a build script that manually uses `os.execute("cp ...")` works but is
tedious.  We embed a Lua prelude that provides helper functions.

The prelude defines helpers like `install_lua(pattern)`,
`install_bin(path)`, `install_share(path, pkg_name)` and sets globals like
`PREFIX`, `SRC_DIR`, etc.  A minimal build script then looks like:

```lua
-- build.lua
install_lua("src/*.lua")
install_bin("scripts/myapp")
```

Here is the complete `src/build_prelude.lua`, assembled from the named sections
that follow:

``` {.lua file=src/build_prelude.lua}
<<prelude-header>>

<<prelude-globals>>

<<prelude-internal-helpers>>

<<prelude-public-api>>

<<prelude-install-helpers>>

<<prelude-done>>
```

The prelude opens with a documentation comment that lists every global and
function available to build scripts. This block is the only documentation a
recipe author needs to consult when writing a `build.lua`.

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
```

The Rust side sets these environment variables before launching the Lua
interpreter (`run_build_script` in `build.rs`). The prelude reads them once
and fails fast if any are missing.

``` {.lua #prelude-globals}
-- ── Globals ───────────────────────────────────────────────────────────────────

PREFIX        = os.getenv("PREFIX")        or error("PREFIX not set")
SRC_DIR       = os.getenv("SRC_DIR")       or error("SRC_DIR not set")
BUILD_PREFIX  = os.getenv("BUILD_PREFIX")  or error("BUILD_PREFIX not set")
PKG_NAME      = os.getenv("PKG_NAME")      or error("PKG_NAME not set")
PKG_VERSION   = os.getenv("PKG_VERSION")   or error("PKG_VERSION not set")
PKG_BUILD_NUM = tonumber(os.getenv("PKG_BUILD_NUM") or "0")
```

Two small utilities that the rest of the prelude depends on. `shell` runs a
command and raises an error on failure; `q` quotes a path so that spaces and
special characters survive the shell.

``` {.lua #prelude-internal-helpers}
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
```

These functions are available to every build script. They cover the most common
file-system operations: joining paths, creating directories, copying, moving,
testing existence, and logging.

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
```

The install helpers build on `shell`, `q`, and `path_join` to give build
scripts a declarative vocabulary. Each function copies files into the right
subdirectory of `PREFIX` so the package layout matches what conda expects.

``` {.lua #prelude-install-helpers}
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
```

Finally, the prelude logs a short banner so the build output shows which
package is being built and where files will land.

``` {.lua #prelude-done}
-- ── Done ──────────────────────────────────────────────────────────────────────

log(string.format("Building %s %s (build %d)", PKG_NAME, PKG_VERSION, PKG_BUILD_NUM))
log(string.format("PREFIX    = %s", PREFIX))
log(string.format("SRC_DIR   = %s", SRC_DIR))
```

### The build command

Here is the full file skeleton for `src/commands/build.rs`, with each section
defined as we encounter it:

``` {.rust file=src/commands/build.rs}
<<build-imports>>

<<build-args>>

<<build-execute>>

<<build-prelude-const>>

<<build-run-script>>

<<build-write-metadata>>

<<build-collect-paths>>

<<build-sha256>>

<<build-pack-conda>>

<<build-find-lua>>
```

#### Imports

``` {.rust #build-imports}
use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_index::{index_fs, IndexFsConfig};
use rattler_conda_types::package::{IndexJson, PackageFile, PathType, PathsEntry, PathsJson};
use rattler_conda_types::{NoArchType, PackageName, VersionWithSource};
use rattler_package_streaming::write::write_conda_package;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::commands::install::install_from_manifest;
use crate::manifest::{Manifest, ProjectMetadata};
use crate::recipe::{Recipe, RECIPE_FILENAME};
```

#### CLI arguments

``` {.rust #build-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Path to `recipe.toml`.  Defaults to `./recipe.toml`.
    #[clap(long)]
    pub recipe: Option<PathBuf>,

    /// Directory where the built `.conda` file is written.
    ///
    /// Defaults to `./output/`.
    #[clap(long, default_value = "output")]
    pub output_dir: PathBuf,
}
```

#### The `execute` function

The `execute` function orchestrates the entire build: it parses the recipe,
creates working directories, installs build dependencies, runs the build script,
writes metadata, packs the archive, and indexes the channel.

``` {.rust #build-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;

    let recipe_path = args
        .recipe
        .clone()
        .unwrap_or_else(|| cwd.join(RECIPE_FILENAME));

    let recipe = Recipe::from_path(&recipe_path)?;
    let recipe_dir = recipe_path.parent().unwrap_or(&cwd).to_path_buf();

    println!(
        "Building {} {} (build {})",
        console::style(&recipe.package.name).cyan(),
        recipe.package.version,
        recipe.build_string(),
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

    // Resolve the source directory.
    let src_dir = {
        let p = PathBuf::from(&recipe.source.path);
        if p.is_absolute() {
            p
        } else {
            recipe_dir.join(&recipe.source.path)
        }
    };
    let src_dir = std::path::absolute(src_dir)
        .into_diagnostic()
        .context("resolving SRC_DIR")?;

    let mut build_deps = recipe.requirements.build.clone();
    // Always ensure lua is available in the build environment.
    if !build_deps.iter().any(|d| d.starts_with("lua")) {
        build_deps.push("lua >=5.1".to_string());
    }

    if !build_deps.is_empty() {
        println!(
            "  {} Installing {} build dependencies…",
            console::style("→").blue(),
            build_deps.len()
        );
        let build_manifest = Manifest {
            project: ProjectMetadata {
                name: format!("{}-build-env", recipe.package.name),
                channels: recipe.channels.list.clone(),
            },
            dependencies: build_deps
                .iter()
                .map(|s| {
                    // Split "name version" into (name, spec) pair
                    let mut parts = s.splitn(2, ' ');
                    let name = parts.next().unwrap_or(s).to_string();
                    let spec = parts.next().unwrap_or("*").to_string();
                    (name, spec)
                })
                .collect(),
        };
        install_from_manifest(&build_manifest, build_prefix.clone()).await?;
    }

    let script_path = recipe_dir.join(&recipe.build.script);
    if !script_path.exists() {
        miette::bail!(
            "Build script `{}` not found (expected at `{}`)",
            recipe.build.script,
            script_path.display()
        );
    }

    let lua_bin = find_lua(&build_prefix)?;

    println!(
        "  {} Running build script `{}`",
        console::style("→").blue(),
        recipe.build.script
    );

    run_build_script(
        &lua_bin,
        &script_path,
        &install_prefix,
        &src_dir,
        &build_prefix,
        &recipe,
    )
    .await?;

    write_package_metadata(&install_prefix, &recipe)
        .context("writing package metadata")?;

    let output_dir = std::path::absolute(&args.output_dir)
        .into_diagnostic()
        .context("resolving output directory")?;

    let subdir_dir = output_dir.join(recipe.subdir());
    std::fs::create_dir_all(&subdir_dir)
        .into_diagnostic()
        .context("creating output subdir")?;

    let filename = recipe.package_filename();
    let output_path = subdir_dir.join(&filename);

    pack_conda(&install_prefix, &output_path, &recipe)?;

    println!(
        "  {} Indexing channel at {}",
        console::style("→").blue(),
        output_dir.display()
    );
    index_fs(IndexFsConfig {
        channel: output_dir.clone(),
        target_platform: None,   // discover all subdirs automatically
        repodata_patch: None,
        write_zst: true,
        write_shards: true,
        force: false,            // incremental — only index new packages
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
}
```

The function sets up two temporary directories:

- **`build_prefix`**: where build-time dependencies are installed.  The Lua
  interpreter lives here.  These never appear in the final package.
- **`install_prefix`**: the "fake root" where the build script installs files.
  Everything in here ends up in the package.

The temporary directory is automatically cleaned up when `work_dir` goes out of
scope.

It constructs a temporary manifest from the recipe's build requirements,
reusing `install_from_manifest` (the same function `shot install` uses) to
install them into the build prefix instead of the project's environment.

#### Build prelude constant

``` {.rust #build-prelude-const}
const BUILD_PRELUDE: &str = include_str!("../build_prelude.lua");
```

#### Running the build script

We locate the Lua interpreter in the build prefix and run the user's build
script through a wrapper that loads the prelude first.  We use a wrapper file
rather than `-e '...'` so that error messages show correct line numbers and the
real filename instead of `<string>`.

The build script can use any tool installed in `build_prefix/bin` because we
prepend it to `PATH`.

``` {.rust #build-run-script}
async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    recipe: &Recipe,
) -> miette::Result<()> {
    let wrapper_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating wrapper temp dir")?;

    let prelude_path = wrapper_dir.path().join("prelude.lua");
    std::fs::write(&prelude_path, BUILD_PRELUDE)
        .into_diagnostic()
        .context("writing build prelude")?;

    // The wrapper dofile()s the prelude then the user script.
    let wrapper_src = format!(
        "dofile({prelude:?})\ndofile({script:?})\n",
        prelude = prelude_path.to_string_lossy(),
        script  = script.to_string_lossy(),
    );
    let wrapper_path = wrapper_dir.path().join("wrapper.lua");
    std::fs::write(&wrapper_path, &wrapper_src)
        .into_diagnostic()
        .context("writing build wrapper")?;

    // Prepend build_prefix/bin to PATH so the script can call any installed
    // build tools (luarocks, make, etc.).
    let build_bin = build_prefix.join("bin");
    let original_path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{original_path}", build_bin.display());

    let status = tokio::process::Command::new(lua_bin)
        .arg(&wrapper_path)
        .env("PREFIX",        install_prefix)
        .env("SRC_DIR",       src_dir)
        .env("BUILD_PREFIX",  build_prefix)
        .env("PKG_NAME",      &recipe.package.name)
        .env("PKG_VERSION",   &recipe.package.version)
        .env("PKG_BUILD_NUM", recipe.package.build_number.to_string())
        .env("PATH",          &new_path)
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

#### Writing package metadata

Every conda package contains an `info/` directory with metadata.  We need two
files: `index.json` and `paths.json`.  The solver reads `IndexJson` from
`conda-meta/*.json` after installation to track what is present in the
environment.

We populate an `IndexJson` struct from the recipe metadata, then walk the
install prefix to build `paths.json`.

``` {.rust #build-write-metadata}
fn write_package_metadata(install_prefix: &Path, recipe: &Recipe) -> miette::Result<()> {
    let info_dir = install_prefix.join("info");
    std::fs::create_dir_all(&info_dir)
        .into_diagnostic()
        .context("creating info/ directory")?;

    let noarch = if recipe.build.noarch {
        NoArchType::generic()
    } else {
        NoArchType::default()
    };

    let subdir = if recipe.build.noarch {
        Some("noarch".to_string())
    } else {
        Some(rattler_conda_types::Platform::current().to_string())
    };

    let index = IndexJson {
        name: PackageName::from_str(&recipe.package.name)
            .into_diagnostic()
            .with_context(|| format!("invalid package name `{}`", recipe.package.name))?,
        version: VersionWithSource::from_str(&recipe.package.version)
            .into_diagnostic()
            .with_context(|| format!("invalid version `{}`", recipe.package.version))?,
        build: recipe.build_string(),
        build_number: recipe.package.build_number,
        subdir,
        arch: None,
        platform: None,
        noarch,
        depends: recipe.requirements.run.clone(),
        constrains: vec![],
        experimental_extra_depends: Default::default(),
        features: None,
        license: recipe.package.license.clone(),
        license_family: None,
        purls: None,
        python_site_packages_path: None,
        track_features: vec![],
        timestamp: Some(
            rattler_conda_types::utils::TimestampMs::from_datetime_millis(chrono::Utc::now())
        ),
    };

    let index_path = install_prefix.join(IndexJson::package_path());
    let index_json = serde_json::to_string_pretty(&index)
        .into_diagnostic()
        .context("serializing index.json")?;
    std::fs::write(&index_path, index_json)
        .into_diagnostic()
        .context("writing info/index.json")?;

    let paths = collect_paths_json(install_prefix)
        .context("building paths.json")?;

    let paths_path = install_prefix.join(PathsJson::package_path());
    let paths_json = serde_json::to_string_pretty(&paths)
        .into_diagnostic()
        .context("serializing paths.json")?;
    std::fs::write(&paths_path, paths_json)
        .into_diagnostic()
        .context("writing info/paths.json")?;

    Ok(())
}
```

#### Collecting paths

We walk the install prefix, hash every file, and record each path in a
`PathsJson` manifest.

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

#### SHA-256 hashing

The SHA-256 hash is computed with the `sha2` crate.  We read the file in 64 KiB
chunks to avoid loading the entire file into memory.

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
produces the final archive.

`rattler_package_streaming::write::write_conda_package` does all the work:

1. Separates `info/` files from payload files.
2. Compresses each group into a `.tar.zst` archive.
3. Wraps both archives and a `metadata.json` into an uncompressed ZIP.

The `.conda` format is designed so that tools can `mmap` the outer ZIP directory
and jump directly to the inner archive they need.

``` {.rust #build-pack-conda}
fn pack_conda(
    install_prefix: &Path,
    output_path: &Path,
    recipe: &Recipe,
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
        recipe.package.name,
        recipe.package.version,
        recipe.build_string()
    );

    let now = chrono::Utc::now();
    write_conda_package(
        writer,
        install_prefix,
        &files,
        CompressionLevel::Default,
        None,      // use all available CPU threads for zstd
        &out_name,
        Some(&now),
        None,      // no progress bar (already shown by our spinner)
    )
    .into_diagnostic()
    .context("writing .conda archive")?;

    Ok(())
}
```

#### Finding the Lua interpreter

``` {.rust #build-find-lua}
fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
    let bin = prefix.join("bin").join("lua");
    if bin.exists() {
        return Ok(bin);
    }
    // Try lua5.4, lua5.3, … as fallbacks
    for minor in (1u8..=4u8).rev() {
        let versioned = prefix.join("bin").join(format!("lua5.{minor}"));
        if versioned.exists() {
            return Ok(versioned);
        }
    }
    miette::bail!(
        "No Lua interpreter found in `{}`. \
         Add `lua` to `[requirements] build` in your recipe.",
        prefix.join("bin").display()
    )
}
```

#### Indexing the channel

After packing, the output directory is not yet a valid conda channel; it has
packages but no `repodata.json`.  The `index_fs` call inside `execute` scans
the directory, reads every `.conda` file's `info/index.json`, and writes:

- `output/noarch/repodata.json`, the plain JSON catalog
- `output/noarch/repodata.json.zst`, a compressed version
- `output/noarch/repodata_shards.msgpack.zst`, the sharded format

Once indexed, the output directory can be used as a channel directly:

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

## Summary

- A `recipe.toml` describes how to build a package.
- Build deps are installed into a temporary prefix; run deps go into `info/index.json`.
- `paths.json` lists every file with its SHA-256 hash.
- `write_conda_package` produces the `.conda` archive format.
- `rattler_index` turns the output directory into a valid conda channel.

With `shot build` working, our package manager is feature-complete.  In Part II
we'll dive deeper into the underlying mechanisms.

[conda-build]: https://docs.conda.io/projects/conda-build/
[rattler-build]: https://github.com/prefix-dev/rattler-build
