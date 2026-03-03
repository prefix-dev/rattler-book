//! # The Manifest
//!
//! Every `luapkg` project is described by a `luapkg.toml` file.  This module
//! is responsible for reading and writing that file.
//!
//! ## Format
//!
//! ```toml
//! [project]
//! name    = "my-lua-app"
//! channels = ["conda-forge"]   # optional, defaults to conda-forge
//!
//! [dependencies]
//! lua      = ">=5.4"
//! luarocks = "*"
//! ```
//!
//! The `[dependencies]` table maps **package names** to **version specs**.
//! Version specs follow the conda MatchSpec mini-language:
//!
//! | Spec          | Meaning                          |
//! |---------------|----------------------------------|
//! | `"*"`         | any version                      |
//! | `">=5.4"`     | 5.4 or newer                     |
//! | `"5.4.*"`     | any 5.4.x release                |
//! | `">=5.4,<6"`  | 5.4 series, exclusive upper bound |
//!
//! We keep the spec as a plain `String` here and let `rattler_conda_types`
//! parse it into a `MatchSpec` later — that way parse errors surface at
//! solve-time with full context rather than at manifest-read time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};

/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "luapkg.toml";

/// A fully-parsed `luapkg.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Metadata about the project itself.
    pub project: ProjectMetadata,

    /// The requested packages and their version constraints.
    ///
    /// Keys are conda package names; values are version spec strings.
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

/// The `[project]` section of `luapkg.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    /// Human-readable project name (used only for display).
    pub name: String,

    /// The conda channels to search for packages, in priority order.
    ///
    /// Defaults to `["conda-forge"]`.
    #[serde(default = "default_channels")]
    pub channels: Vec<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}

impl Manifest {
    /// Read a manifest from disk.
    ///
    /// `path` should point to the `luapkg.toml` file itself (not its parent
    /// directory).
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .with_context(|| format!("reading manifest at `{}`", path.display()))?;

        toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))
    }

    /// Write the manifest back to disk, overwriting any existing file.
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .context("serializing manifest")?;

        std::fs::write(path, content)
            .into_diagnostic()
            .with_context(|| format!("writing manifest to `{}`", path.display()))
    }

    /// Find the `luapkg.toml` in the given directory and parse it.
    pub fn find_in_dir(dir: &Path) -> miette::Result<(PathBuf, Self)> {
        let path = dir.join(MANIFEST_FILENAME);
        if !path.exists() {
            miette::bail!(
                "No `{MANIFEST_FILENAME}` found in `{}`. \
                 Run `luapkg init` to create one.",
                dir.display()
            );
        }
        let manifest = Self::from_path(&path)?;
        Ok((path, manifest))
    }
}
