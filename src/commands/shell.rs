//! # `luapkg shell`
//!
//! Prints a shell activation script that, when evaluated, modifies the current
//! shell's environment so that installed packages are on `PATH`.
//!
//! ## Usage
//!
//! ```bash
//! # bash / zsh
//! eval $(luapkg shell)
//!
//! # fish
//! luapkg shell | source
//! ```
//!
//! ## How activation works
//!
//! When conda installs packages into a prefix it also writes activation
//! metadata into two well-known locations:
//!
//! * `<prefix>/etc/conda/activate.d/`   — shell scripts to *source* on activate
//! * `<prefix>/etc/conda/deactivate.d/` — shell scripts to *source* on deactivate
//! * `<prefix>/conda-meta/state`         — JSON env-var overrides
//! * `<prefix>/etc/conda/env_vars.d/`   — additional env-var JSON files
//!
//! `rattler_shell::activation::Activator` reads all of these and produces a
//! single shell script that:
//!   1. Prepends `<prefix>/bin` (and siblings on Windows) to `PATH`.
//!   2. Sets `CONDA_PREFIX` to the prefix path.
//!   3. Tracks nesting depth in `CONDA_SHLVL` (so nested activations compose).
//!   4. Sources any `activate.d` scripts.
//!   5. Applies extra environment variables from `env_vars.d`.

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

    // Resolve which shell to emit.
    //
    // Priority: --shell flag > $SHELL env var > bash (safe default)
    let shell: ShellEnum = if let Some(ref name) = args.shell {
        ShellEnum::from_str(name)
            .map_err(|_| miette::miette!("Unknown shell `{name}`. Try: bash, zsh, fish"))?
    } else {
        ShellEnum::from_env().unwrap_or_else(|| Bash.into())
    };

    // Build the Activator.
    //
    // `Activator::from_path` inspects the prefix directory and discovers:
    //   • `paths`               — directories to prepend to PATH
    //   • `activation_scripts`  — scripts in etc/conda/activate.d/
    //   • `env_vars`            — variables from conda-meta/state
    let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;

    // Collect the current shell state so the activator can deactivate any
    // previously-activated environment and handle CONDA_SHLVL correctly.
    let vars = ActivationVariables::from_env().into_diagnostic()?;

    // Generate the activation script.  This is a plain text shell script;
    // the user evaluates it with `eval $(luapkg shell)`.
    let result = activator.activation(vars).into_diagnostic()?;
    let script = result.script.contents().into_diagnostic()?;

    print!("{script}");
    Ok(())
}
