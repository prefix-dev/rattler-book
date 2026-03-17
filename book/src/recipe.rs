// ~/~ begin <<src/ch09-build.md#src/recipe.rs>>[init]
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};

/// File name we look for in the current directory.
pub const RECIPE_FILENAME: &str = "recipe.toml";

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
// ~/~ end
