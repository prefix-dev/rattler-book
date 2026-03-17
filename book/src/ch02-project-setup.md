# Chapter 2: Setting Up the Project

In this chapter we create the Rust project, add our dependencies, and build the
CLI skeleton with Clap.  By the end of the chapter `luapkg --help` works, even
though none of the commands do anything yet.

## Creating the project

```
cargo new luapkg
cd luapkg
```

## Dependencies

Open `Cargo.toml` and add the following.  We'll explain each crate as we use it.

```toml
[package]
name    = "luapkg"
version = "0.1.0"
edition = "2021"

[dependencies]
# CLI argument parsing
clap = { version = "4", features = ["derive"] }

# Error handling, user-facing diagnostics
miette  = { version = "7", features = ["fancy"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# Config file format
toml  = "0.8"
serde = { version = "1", features = ["derive"] }

# Pretty terminal output
console    = "0.15"
indicatif  = "0.17"

# Logging
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# HTTP
reqwest            = { version = "0.12", default-features = false, features = ["rustls-tls"] }
reqwest-middleware = "0.3"

# rattler crates
rattler                    = { version = "0.28" }
rattler_cache              = { version = "0.1" }
rattler_conda_types        = { version = "0.28" }
rattler_networking         = { version = "0.21" }
rattler_repodata_gateway   = { version = "0.22" }
rattler_shell              = { version = "0.22" }
rattler_solve              = { version = "0.28" }
rattler_virtual_packages   = { version = "0.21" }
rattler_package_streaming  = { version = "0.22" }
rattler_index              = { version = "0.2" }
rattler_digest             = { version = "0.19" }

# Build command helpers
sha2      = "0.10"
chrono    = { version = "0.4", features = ["serde"] }
walkdir   = "2"
tempfile  = "3"
serde_json = "1"
```

The rattler crates (`rattler`, `rattler_solve`, `rattler_shell`, etc.) implement the conda specification. The rest of the dependency list is general-purpose infrastructure: `clap` for CLI parsing, `tokio` for async I/O, `reqwest` for HTTP, and so on.

> **Why so many rattler crates?**  rattler is split into fine-grained crates so
> you can depend on only the parts you need.  A tool that only needs to solve
> dependencies doesn't have to pull in the HTTP stack.  We use most of them, so
> the list is long.

Package managers surface errors from many sources (network, filesystem, solver conflicts, malformed metadata), so `miette` with `features = ["fancy"]` is worth pulling in early. It renders structured diagnostics with source spans, which makes dependency conflicts and parse errors much easier to read than a plain error string.

Notice that `reqwest` uses `default-features = false, features = ["rustls-tls"]`. This selects the pure-Rust TLS implementation instead of linking against the system's OpenSSL, so the binary builds and runs on any platform without requiring a system TLS library.

## The entry point: `src/main.rs`

Here is how the project is structured:

```
src/
├── main.rs          ← CLI wiring, Tokio runtime
├── manifest.rs      ← luapkg.toml parser
├── recipe.rs        ← recipe.toml parser
├── progress.rs      ← spinner helpers
└── commands/
    ├── mod.rs       ← shared helpers (prefix_dir)
    ├── init.rs
    ├── add.rs
    ├── install.rs
    ├── shell.rs
    ├── run.rs
    └── build.rs
```

Here is the complete `main.rs`:

```rust
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
    Shell(commands::shell::Args),

    /// Run a command inside the activated environment.
    Run(commands::run::Args),

    /// Build a Lua package from a recipe.toml.
    Build(commands::build::Args),
}

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
        Command::Init(args)    => commands::init::execute(args).await,
        Command::Add(args)     => commands::add::execute(args).await,
        Command::Install(args) => commands::install::execute(args).await,
        Command::Shell(args)   => commands::shell::execute(args),
        Command::Run(args)     => commands::run::execute(args).await,
        Command::Build(args)   => commands::build::execute(args).await,
    }
}
```

The runtime configuration splits available CPU cores between two pools. `worker_threads` handles async work (HTTP requests, futures polling), while `max_blocking_threads` handles synchronous operations that would stall the async scheduler (file I/O, archive extraction, solver runs). Giving the blocking pool more threads prevents slow disk operations from starving network requests.

Let's take it section by section.

## Logging with tracing

```rust
tracing_subscriber::fmt()
    .with_env_filter(env_filter)
    .without_time()
    .with_writer(std::io::stderr)
    .init();
```

We route all log output to stderr, leaving stdout clean for machine-readable
output (like `luapkg shell`, which prints a shell script).  The `--verbose` flag
raises the log level to `DEBUG`; users can also set `RUST_LOG=debug` for more
control.

## Summary

- We set up a Rust project with a clean module tree.
- Clap's derive macros turn struct definitions into a full CLI parser.
- Tokio provides the async runtime; we configure two thread pools.
- `?` and `miette` handle error propagation and display.

In the next chapter we implement the simplest command: `luapkg init`, which
writes the project manifest.
