//! # `luapkg run`
//!
//! Runs an arbitrary command inside the activated environment without requiring
//! the user to call `eval $(luapkg shell)` first.
//!
//! ## Example
//!
//! ```text
//! $ luapkg run lua -e 'print("hello from conda")'
//! hello from conda
//! ```
//!
//! ## Implementation
//!
//! We use `Activator::run_activation` to execute a small shell script that:
//!   1. Prints all current environment variables (the "before" snapshot).
//!   2. Runs the conda activation logic.
//!   3. Prints all environment variables again (the "after" snapshot).
//!
//! rattler then diffs the two snapshots and returns only the *changed* vars.
//! We inject those into the child process environment so the command runs as
//! if the environment had been activated, without touching the parent shell.
//!
//! This pattern is the same one pixi uses for `pixi run`.

use std::env;
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};
use tokio::process::Command;

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

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    if !prefix.exists() {
        miette::bail!(
            "Environment not found at `{}`. Run `luapkg install` first.",
            prefix.display()
        );
    }

    let platform = Platform::current();
    let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());

    // ── Build the activated environment ──────────────────────────────────────
    //
    // `run_activation` is a synchronous call that:
    //   1. Writes a temporary shell script that:
    //        a. Emits current env vars (via `env` or `set`)
    //        b. Sources the activation logic
    //        c. Emits env vars again
    //   2. Executes that script with the user's shell.
    //   3. Diffs the two env-var snapshots.
    //   4. Returns the diff as `HashMap<String, String>`.
    //
    // This means `run` works correctly even if packages install custom
    // activate.d scripts that set env vars dynamically (e.g., PKG_CONFIG_PATH,
    // LUA_PATH, etc.).
    let activator =
        Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
    let current_vars = ActivationVariables::from_env().into_diagnostic()?;

    // run_activation is sync (it shells out), run it on a blocking thread so
    // we don't block the tokio executor.
    let activation_env = tokio::task::spawn_blocking(move || {
        activator.run_activation(current_vars, None)
    })
    .await
    .into_diagnostic()? // JoinError
    .into_diagnostic()?; // ActivationError

    // ── Spawn the user's command with the activated env ───────────────────────
    let (program, rest_args) = args.command.split_first().expect("clap ensures non-empty");

    let status = Command::new(program)
        .args(rest_args)
        // Inherit the current env first, then overlay the activated vars.
        .envs(&activation_env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .into_diagnostic()?;

    // Propagate the child's exit code.
    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }

    Ok(())
}
