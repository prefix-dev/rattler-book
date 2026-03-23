// ~/~ begin <<book/src/ch06-lock.md#src/commands/lock.rs>>[init]
// ~/~ begin <<book/src/ch06-lock.md#lock-cmd-imports>>[init]
use std::env;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;

use crate::lock::{is_lock_fresh, write_lock_file, LOCK_FILENAME};
use crate::manifest::Manifest;
use crate::resolve::{read_locked_packages, resolve_from_manifest};
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
    let cwd = env::current_dir().into_diagnostic()?;
    let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;
    let lock_path = cwd.join(LOCK_FILENAME);

    if !args.force && is_lock_fresh(&lock_path, &manifest_path) {
        println!("{} Lock is already up to date", console::style("✔").green());
        return Ok(());
    }

    let platform = Platform::current();
    let existing = read_locked_packages(&lock_path, platform);
    let (solution, channels, platform) = resolve_from_manifest(&manifest, existing).await?;

    write_lock_file(&lock_path, &channels, platform, &solution)?;

    println!(
        "{} Wrote {} ({} packages)",
        console::style("✔").green(),
        LOCK_FILENAME,
        console::style(solution.len()).cyan()
    );

    Ok(())
}
// ~/~ end
// ~/~ end
