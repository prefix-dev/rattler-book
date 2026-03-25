// ~/~ begin <<book/src/ch07-install.md#src/commands/install.rs>>[init]
// ~/~ begin <<book/src/ch07-install.md#install-imports>>[init]
use clap::Parser;
use miette::{Context, IntoDiagnostic};

use crate::lock::LOCK_FILENAME;
use crate::project::Project;
use crate::session::{ResolveStatus, Session};
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let session = Session::new(project)?;

    let prefix = args
        .prefix
        .unwrap_or_else(|| session.project.default_prefix());
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    let status = session.ensure_resolved(false).await?;

    match &status {
        ResolveStatus::AlreadyFresh(_) => {}
        ResolveStatus::Resolved { solution, .. } => {
            println!(
                "{} Wrote {} ({} packages)",
                console::style("✔").green(),
                LOCK_FILENAME,
                console::style(solution.len()).cyan()
            );
        }
    }

    let platform = rattler_conda_types::Platform::current();
    session
        .install_packages(&prefix, status.into_solution(), platform)
        .await
}
// ~/~ end
// ~/~ end
