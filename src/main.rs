//! # luapkg — a minimal Lua package manager
//!
//! `luapkg` is the worked example that accompanies *The Rattler Book*.  It
//! installs Lua packages from conda channels (primarily `conda-forge`) into an
//! isolated prefix, and knows how to activate that prefix so your shell can
//! find the installed Lua interpreter and libraries.
//!
//! ## Commands
//!
//! | Command         | What it does                                       |
//! |-----------------|----------------------------------------------------|
//! | `init [name]`   | Create a new `luapkg.toml` in the current dir      |
//! | `add <pkgs…>`   | Add packages to the manifest and install them      |
//! | `install`       | Install (or update) all packages in the manifest   |
//! | `shell`         | Print a shell activation script (eval-able)        |
//! | `run <cmd>`     | Run a command inside the activated environment     |
//!
//! ## Architecture overview
//!
//! ```text
//!  luapkg.toml  ──▶  Manifest::from_path
//!                           │
//!                           ▼
//!               rattler_repodata_gateway::Gateway
//!               (fetches & caches repodata.json)
//!                           │
//!                           ▼
//!               rattler_solve (resolvo SAT solver)
//!               (picks a consistent set of packages)
//!                           │
//!                           ▼
//!               rattler::Installer
//!               (downloads, extracts, hard-links)
//!                           │
//!                           ▼
//!               .luapkg/env/   (the conda prefix)
//!                           │
//!                           ▼
//!               rattler_shell::Activator
//!               (generate activation script)
//! ```

use clap::Parser;
use miette::IntoDiagnostic;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

mod commands;
mod manifest;
mod progress;
mod recipe;

/// A minimal Lua package manager powered by rattler.
#[derive(Debug, Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Command,

    /// Enable verbose logging.
    #[clap(short, long, global = true)]
    verbose: bool,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Create a new luapkg.toml in the current directory.
    Init(commands::init::Args),

    /// Add one or more packages to the manifest and install them.
    Add(commands::add::Args),

    /// Install (or update) all packages listed in luapkg.toml.
    Install(commands::install::Args),

    /// Print a shell activation script.
    ///
    /// Evaluate the output in your shell to activate the environment:
    ///
    ///   eval $(luapkg shell)          # bash / zsh
    ///   luapkg shell | source         # fish
    Shell(commands::shell::Args),

    /// Run a command inside the activated environment.
    Run(commands::run::Args),

    /// Build a Lua package from a recipe.toml.
    ///
    /// Reads a `recipe.toml` in the current directory, installs build
    /// dependencies, runs the Lua build script, and packs the result into a
    /// `.conda` archive ready for distribution.
    Build(commands::build::Args),
}

fn main() -> miette::Result<()> {
    // Build a multi-threaded Tokio runtime.  We give blocking threads up to
    // `num_cpus` slots so that parallel package extraction doesn't stall.
    let num_cpus = std::thread::available_parallelism()
        .map_or(2, std::num::NonZero::get)
        .max(2);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_cpus / 2)
        .max_blocking_threads(num_cpus)
        .enable_all()
        .build()
        .into_diagnostic()?;

    runtime.block_on(async_main())
}

async fn async_main() -> miette::Result<()> {
    let cli = Cli::parse();

    // Configure logging.  The user can override the level with RUST_LOG; the
    // --verbose flag raises the default to DEBUG.
    let default_level = if cli.verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::WARN
    };
    let env_filter = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env()
        .into_diagnostic()?;

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Command::Init(args) => commands::init::execute(args).await,
        Command::Add(args) => commands::add::execute(args).await,
        Command::Install(args) => commands::install::execute(args).await,
        Command::Shell(args) => commands::shell::execute(args),
        Command::Run(args) => commands::run::execute(args).await,
        Command::Build(args) => commands::build::execute(args).await,
    }
}
