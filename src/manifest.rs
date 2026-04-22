// ~/~ begin <<book/src/ch03-init.md#src/manifest.rs>>[init]
// ~/~ begin <<book/src/ch03-init.md#manifest-imports>>[init]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{MatchSpec, NamelessMatchSpec, PackageName, Platform};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-filename-const>>[init]
/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "moonshot.toml";
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-structs>>[init]
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMetadata,
    #[serde_as(as = "BTreeMap<_, DisplayFromStr>")]
    #[serde(default)]
    pub dependencies: BTreeMap<String, NamelessMatchSpec>,

    /// Present only for library projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,
}
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-structs>>[1]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,

    #[serde(default = "default_channels")]
    pub channels: Vec<String>,

    #[serde(default = "default_platforms")]
    pub platforms: Vec<Platform>,

    /// Package version (required when [build] is present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// SPDX license identifier, e.g. "MIT".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// One-line package description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}

pub(crate) fn default_platforms() -> Vec<Platform> {
    vec![Platform::current()]
}
// ~/~ end
// ~/~ begin <<book/src/ch10-build.md#manifest-structs>>[0]
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
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-impl>>[init]
impl Manifest {
// ~/~ begin <<book/src/ch03-init.md#manifest-from-path>>[init]
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = fs::read_to_string(path)
            .into_diagnostic()
            .context("reading manifest")?;

        // `DisplayFromStr` validates every dependency spec during
        // deserialization, so a typo like `">==5.4"` fails here.
        let manifest: Self = toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))?;

        Ok(manifest)
    }
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-write>>[init]
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .context("serializing manifest")?;

        fs::write(path, content)
            .into_diagnostic()
            .context("writing manifest")
    }
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-find-in-dir>>[init]
    pub fn find_in_dir(dir: &Path) -> miette::Result<(PathBuf, Self)> {
        let path = dir.join(MANIFEST_FILENAME);
        if !path.exists() {
            miette::bail!(
                "No `{MANIFEST_FILENAME}` found in `{}`. \
                 Run `shot init` to create one.",
                dir.display()
            );
        }
        let manifest = Self::from_path(&path)?;
        Ok((path, manifest))
    }
// ~/~ end
// ~/~ begin <<book/src/ch10-build.md#manifest-build-helpers>>[init]
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
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-spec-helpers>>[init]
    /// Combine each `[dependencies]` entry into a full [`MatchSpec`].
    ///
    /// The values are already parsed `NamelessMatchSpec`s, so this just
    /// attaches the package name.
    pub fn match_specs(&self) -> miette::Result<Vec<MatchSpec>> {
        self.dependencies
            .iter()
            .map(|(name, spec)| {
                let name = PackageName::from_str(name)
                    .into_diagnostic()
                    .with_context(|| format!("invalid package name `{name}`"))?;
                Ok(MatchSpec::from_nameless(spec.clone(), name.into()))
            })
            .collect()
    }
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#manifest-spec-helpers>>[1]
    /// Format dependencies as `"name version"` strings (or just `"name"`
    /// when there is no version constraint).
    ///
    /// This is the format expected by conda's `index.json` `depends` field.
    pub fn dependency_strings(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .map(|(name, spec)| {
                if spec.version.is_none() {
                    name.clone()
                } else {
                    format!("{name} {spec}")
                }
            })
            .collect()
    }
// ~/~ end
}
// ~/~ end
// ~/~ end
