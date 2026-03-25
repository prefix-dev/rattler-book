// ~/~ begin <<book/src/ch08-shell-hook.md#src/commands/shell.rs>>[init]
use clap::Parser;

use crate::environment::Environment;
use crate::project::Project;

#[derive(Debug, Parser)]
pub struct Args {
    /// Shell dialect to emit.  Auto-detected from $SHELL if not set.
    ///
    /// Supported values: bash, zsh, fish, xonsh, powershell, cmd, nushell
    #[clap(long)]
    pub shell: Option<String>,

    /// Override the prefix path.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

pub fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let env = Environment::from_project(&project, args.prefix)?;
    env.ensure_exists()?;

    let script = env.activate_script(args.shell.as_deref())?;
    print!("{script}");
    Ok(())
}
// ~/~ end
