// ~/~ begin <<book/src/ch02-project-setup.md#src/commands/mod.rs>>[init]
pub mod add;
pub mod build;
pub mod init;
pub mod install;
pub mod lock;
pub mod run;
pub mod search;
pub mod shell;

use std::path::{Path, PathBuf};

/// Return the path to the conda prefix managed by `moonshot`.
///
/// By convention we store the environment at `.env/` relative to the
/// project root (the directory that contains `moonshot.toml`).  This is similar
/// to how pixi stores its environments in `.pixi/envs/`.
pub fn prefix_dir(project_root: &Path) -> PathBuf {
    project_root.join(".env")
}
// ~/~ end
