// ~/~ begin <<book/src/ch02-project-setup.md#src/main.rs>>[init]
// ~/~ begin <<book/src/ch02-project-setup.md#main-imports>>[init]
use clap::Parser;
use miette::IntoDiagnostic;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

mod build_backend;
mod client;
mod commands;
mod environment;
mod lock;
mod manifest;
mod progress;
mod project;
mod session;
// ~/~ end
// ~/~ begin <<book/src/ch02-project-setup.md#main-cli-struct>>[init]
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
    /// Create a new moonshot.toml in the current directory.
    Init(commands::init::Args),

    /// Search for packages in a channel.
    Search(commands::search::Args),

    /// Add one or more packages to the manifest.
    Add(commands::add::Args),

    /// Install (or update) all packages listed in moonshot.toml.
    Install(commands::install::Args),

    /// Resolve dependencies and write moonshot.lock.
    Lock(commands::lock::Args),

    /// Print a shell activation script.
    ShellHook(commands::shell_hook::Args),

    /// Run a command inside the activated environment.
    Run(commands::run::Args),

    /// Build a .conda package from the current project.
    Build(commands::build::Args),
}
// ~/~ end
// ~/~ begin <<book/src/ch02-project-setup.md#main-fn>>[init]
fn main() -> miette::Result<()> {
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
// ~/~ end
// ~/~ begin <<book/src/ch02-project-setup.md#main-async>>[init]
async fn async_main() -> miette::Result<()> {
    let cli = Cli::parse();

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
        Command::Search(args) => commands::search::execute(args).await,
        Command::Add(args) => commands::add::execute(args).await,
        Command::Install(args) => commands::install::execute(args).await,
        Command::Lock(args) => commands::lock::execute(args).await,
        Command::ShellHook(args) => commands::shell_hook::execute(args),
        Command::Run(args) => commands::run::execute(args).await,
        Command::Build(args) => commands::build::execute(args).await,
    }
}
// ~/~ end
// ~/~ end
