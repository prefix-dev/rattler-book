// ~/~ begin <<book/src/ch07-shell.md#src/commands/shell.rs>>[init]
use std::env;
use std::str::FromStr;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};

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
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    if !prefix.exists() {
        miette::bail!(
            "Environment not found at `{}`. Run `luapkg install` first.",
            prefix.display()
        );
    }

    let platform = Platform::current();

    let shell: ShellEnum = if let Some(ref name) = args.shell {
        ShellEnum::from_str(name)
            .map_err(|_| miette::miette!("Unknown shell `{name}`. Try: bash, zsh, fish"))?
    } else {
        ShellEnum::from_env().unwrap_or_else(|| Bash.into())
    };

    let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;

    let vars = ActivationVariables::from_env().into_diagnostic()?;

    let result = activator.activation(vars).into_diagnostic()?;
    let script = result.script.contents().into_diagnostic()?;

    print!("{script}");
    Ok(())
}
// ~/~ end
