// ~/~ begin <<book/src/ch09-run.md#src/commands/run.rs>>[init]
// ~/~ begin <<book/src/ch09-run.md#run-imports>>[init]
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use tokio::process::Command;

use crate::environment::Environment;
use crate::project::Project;
// ~/~ end
// ~/~ begin <<book/src/ch09-run.md#run-args>>[init]
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
// ~/~ begin <<book/src/ch09-run.md#run-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let env = Environment::from_project(&project, args.prefix)?;
    env.ensure_exists()?;

    let activation_env = env.activation_env().await?;

    let (program, rest_args) = args.command.split_first().expect("clap ensures non-empty");
// ~/~ end
// ~/~ begin <<book/src/ch09-run.md#run-execute>>[1]
    let status = Command::new(program)
        .args(rest_args)
        .envs(&activation_env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .into_diagnostic()?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }

    Ok(())
}
// ~/~ end
// ~/~ end
