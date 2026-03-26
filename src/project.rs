// ~/~ begin <<book/src/ch05-add.md#src/project.rs>>[init]
// ~/~ begin <<book/src/ch05-add.md#project-imports>>[init]
use crate::manifest::Manifest;
use miette::IntoDiagnostic;
use std::path::PathBuf;
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#project-struct>>[init]
/// A discovered project on disk.
///
/// Bundles the project root, manifest path, and parsed manifest into a
/// single value so that every command does not have to repeat the same
/// discovery boilerplate.
pub struct Project {
    /// The directory that contains `moonshot.toml`.
    pub root: PathBuf,
    /// Absolute path to `moonshot.toml`.
    pub manifest_path: PathBuf,
    /// The parsed manifest.
    pub manifest: Manifest,
}
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#project-impl>>[init]
impl Project {
    /// Discover the project from the current working directory.
    pub fn discover() -> miette::Result<Self> {
        let cwd = std::env::current_dir().into_diagnostic()?;
        let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;
        let root = manifest_path
            .parent()
            .expect("manifest path always has a parent")
            .to_path_buf();
        Ok(Self {
            root,
            manifest_path,
            manifest,
        })
    }

    /// The default prefix directory for the conda environment.
    pub fn default_prefix(&self) -> PathBuf {
        self.root.join(".env")
    }

    /// Persist the (possibly modified) manifest back to disk.
    pub fn save(&self) -> miette::Result<()> {
        self.manifest.write(&self.manifest_path)
    }
}
// ~/~ end
// ~/~ end
// ~/~ begin <<book/src/ch06-lock.md#src/project.rs>>[0]
// ~/~ begin <<book/src/ch06-lock.md#project-lock-imports>>[init]
use crate::lock::{is_lock_fresh, LOCK_FILENAME};
// ~/~ end
// ~/~ begin <<book/src/ch06-lock.md#project-lock-methods>>[init]
impl Project {
    /// Path to the lock file (`moonshot.lock`).
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILENAME)
    }

    /// Returns `true` when the lock file exists and is newer than the
    /// manifest, meaning the solver does not need to run again.
    pub fn is_lock_fresh(&self) -> bool {
        is_lock_fresh(&self.lock_path(), &self.manifest_path)
    }
}
// ~/~ end
// ~/~ end
