// ~/~ begin <<book/src/ch08-run.md#src/commands/run.rs>>[init]
// ~/~ begin <<book/src/ch08-run.md#run-imports>>[init]
use std::env;
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};
use tokio::process::Command;
// ~/~ end

// ~/~ begin <<book/src/ch08-run.md#run-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// The command to run (and its arguments).
    ///
    /// Everything after `run` is passed verbatim to the OS.
    #[clap(required = true, trailing_var_arg = true)]
    pub command: Vec<String>,

    /// Override the prefix path.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
// ~/~ end

// ~/~ begin <<book/src/ch08-run.md#run-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch08-run.md#run-setup>>[init]
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    if !prefix.exists() {
        miette::bail!(
            "Environment not found at `{}`. Run `shot install` first.",
            prefix.display()
        );
    }

    let platform = Platform::current();
    let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());

    let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
    let current_vars = ActivationVariables::from_env().into_diagnostic()?;
    // ~/~ end

    // ~/~ begin <<book/src/ch08-run.md#run-activation>>[init]
    let activation_env =
        tokio::task::spawn_blocking(move || activator.run_activation(current_vars, None))
            .await
            .into_diagnostic()?
            .into_diagnostic()?;
    // ~/~ end

    // ~/~ begin <<book/src/ch08-run.md#run-spawn>>[init]
    let (program, rest_args) = args.command.split_first().expect("clap ensures non-empty");

    let status = Command::new(program)
        .args(rest_args)
        .envs(&activation_env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .into_diagnostic()?;
    // ~/~ end

    // ~/~ begin <<book/src/ch08-run.md#run-exit-code>>[init]
    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }

    Ok(())
    // ~/~ end
}
// ~/~ end
// ~/~ end
