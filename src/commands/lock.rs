// ~/~ begin <<book/src/ch06-lock.md#src/commands/lock.rs>>[init]
// ~/~ begin <<book/src/ch06-lock.md#lock-cmd-imports>>[init]
use clap::Parser;

use crate::lock::LOCK_FILENAME;
use crate::project::Project;
use crate::session::{ResolveStatus, Session};
// ~/~ end
// ~/~ begin <<book/src/ch06-lock.md#lock-cmd-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Force re-solving even if the lock is up to date.
    #[clap(long)]
    pub force: bool,
}
// ~/~ end
// ~/~ begin <<book/src/ch06-lock.md#lock-cmd-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let session = Session::new(project)?;

    match session.ensure_resolved(args.force).await? {
        ResolveStatus::AlreadyFresh(_) => {
            println!("{} Lock is already up to date", console::style("✔").green());
        }
        ResolveStatus::Resolved { ref solution, .. } => {
            println!(
                "{} Wrote {} ({} packages)",
                console::style("✔").green(),
                LOCK_FILENAME,
                console::style(solution.len()).cyan()
            );
        }
    }

    Ok(())
}
// ~/~ end
// ~/~ end
