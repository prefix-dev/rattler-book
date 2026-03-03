//! # The Recipe
//!
//! A `recipe.toml` describes how to *build* a Lua package.  It is the
//! equivalent of `rattler-build`'s `recipe.yaml`, deliberately stripped down
//! to the essentials.
//!
//! ## Minimal example
//!
//! ```toml
//! [package]
//! name    = "moonshine"
//! version = "0.3.0"
//!
//! [requirements]
//! run = ["lua >=5.4"]
//!
//! [build]
//! script = "build.lua"
//! ```
//!
//! ## Build script environment variables
//!
//! The Lua interpreter is executed with these variables set:
//!
//! | Variable         | Meaning                                     |
//! |------------------|---------------------------------------------|
//! | `PREFIX`         | Directory where files should be installed   |
//! | `SRC_DIR`        | Root of the source directory                |
//! | `BUILD_PREFIX`   | Prefix where build-time deps are installed  |
//! | `PKG_NAME`       | Package name from recipe                    |
//! | `PKG_VERSION`    | Package version from recipe                 |
//! | `PKG_BUILD_NUM`  | Build number (integer)                      |
//!
//! ## Typical build.lua
//!
//! ```lua
//! local prefix  = os.getenv("PREFIX")
//! local src     = os.getenv("SRC_DIR")
//!
//! -- Install all .lua files into the Lua 5.4 package path
//! local dest = prefix .. "/share/lua/5.4/"
//! os.execute("mkdir -p " .. dest)
//! os.execute("cp " .. src .. "/*.lua " .. dest)
//! ```
//!
//! ## noarch packages
//!
//! Lua is an interpreted language: the same `.lua` file runs on every
//! platform.  Set `noarch = true` in `[build]` (the default) to mark the
//! package as architecture-independent.  `luapkg` will set `subdir = "noarch"`
//! in the produced `info/index.json` and place the package into the
//! `noarch/` subdirectory of the channel.

use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};

/// File name we look for in the current directory.
pub const RECIPE_FILENAME: &str = "recipe.toml";

// ── Top-level recipe ──────────────────────────────────────────────────────────

/// A fully-parsed `recipe.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// Package identity.
    pub package: PackageMeta,

    /// Source code location.
    #[serde(default)]
    pub source: SourceSpec,

    /// Conda channels to pull build/run dependencies from.
    #[serde(default)]
    pub channels: ChannelSpec,

    /// Dependency requirements.
    #[serde(default)]
    pub requirements: Requirements,

    /// Build configuration.
    #[serde(default)]
    pub build: BuildConfig,
}

// ── [package] ─────────────────────────────────────────────────────────────────

/// The `[package]` section — package identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    /// The conda package name (lowercase, hyphens allowed).
    pub name: String,

    /// Semantic version string, e.g. `"1.2.3"`.
    pub version: String,

    /// Monotonically-increasing integer; increment on rebuilds of the same
    /// version (different compiler flags, patched deps, …).
    #[serde(default)]
    pub build_number: u64,

    /// SPDX license identifier, e.g. `"MIT"` or `"Apache-2.0"`.
    pub license: Option<String>,

    /// One-line description shown in `luapkg search`.
    pub description: Option<String>,
}

// ── [source] ──────────────────────────────────────────────────────────────────

/// The `[source]` section — where to find the source files.
///
/// Defaults to the current directory if omitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// Path to the source tree.  Can be absolute or relative to the recipe.
    ///
    /// Defaults to `"."` (the directory containing `recipe.toml`).
    #[serde(default = "dot")]
    pub path: String,
}

fn dot() -> String {
    ".".to_string()
}

// ── [channels] ────────────────────────────────────────────────────────────────

/// Which conda channels to search when installing build/run dependencies.
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

// ── [requirements] ────────────────────────────────────────────────────────────

/// The `[requirements]` section — what must be present to build and to run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Requirements {
    /// Packages installed into the *build* prefix before the build script
    /// runs.  They do NOT end up in the final package.
    #[serde(default)]
    pub build: Vec<String>,

    /// Packages the user must have installed at runtime.  These are recorded
    /// in `info/index.json` and enforced by the solver at install time.
    #[serde(default)]
    pub run: Vec<String>,
}

// ── [build] ───────────────────────────────────────────────────────────────────

/// The `[build]` section — how to actually build the package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Path to the Lua build script, relative to the recipe directory.
    ///
    /// The script is executed with `lua <script>` inside the activated *build*
    /// prefix.  Defaults to `"build.lua"`.
    #[serde(default = "default_script")]
    pub script: String,

    /// Mark the package as `noarch: generic`.
    ///
    /// A noarch package contains no compiled code and works on every platform.
    /// Pure Lua libraries should almost always set this to `true` (the
    /// default).  Set to `false` if your package embeds binary extensions.
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

// ── I/O helpers ───────────────────────────────────────────────────────────────

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
