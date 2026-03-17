// ~/~ begin <<src/ch06-install.md#src/commands/mod.rs>>[init]
pub mod add;
pub mod build;
pub mod init;
pub mod install;
pub mod run;
pub mod shell;

use std::path::{Path, PathBuf};

/// Return the path to the conda prefix managed by `luapkg`.
///
/// By convention we store the environment at `.luapkg/env/` relative to the
/// project root (the directory that contains `luapkg.toml`).  This is similar
/// to how pixi stores its environments in `.pixi/envs/`.
pub fn prefix_dir(project_root: &Path) -> PathBuf {
    project_root.join(".luapkg").join("env")
}
// ~/~ end
