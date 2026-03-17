// ~/~ begin <<src/ch03-manifest.md#src/manifest.rs>>[init]
// ~/~ begin <<src/ch03-manifest.md#manifest-imports>>[init]
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
// ~/~ end

// ~/~ begin <<src/ch03-manifest.md#manifest-filename-const>>[init]
/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "luapkg.toml";
// ~/~ end

// ~/~ begin <<src/ch03-manifest.md#manifest-structs>>[init]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMetadata,

    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,

    #[serde(default = "default_channels")]
    pub channels: Vec<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}
// ~/~ end

// ~/~ begin <<src/ch03-manifest.md#manifest-impl>>[init]
impl Manifest {
// ~/~ begin <<src/ch03-manifest.md#manifest-from-path>>[init]
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .with_context(|| format!("reading manifest at `{}`", path.display()))?;

        toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))
    }
// ~/~ end

// ~/~ begin <<src/ch03-manifest.md#manifest-write>>[init]
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .context("serializing manifest")?;

        std::fs::write(path, content)
            .into_diagnostic()
            .with_context(|| format!("writing manifest to `{}`", path.display()))
    }
// ~/~ end

// ~/~ begin <<src/ch03-manifest.md#manifest-find-in-dir>>[init]
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
// ~/~ end
}
// ~/~ end
// ~/~ end
