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

# Error handling — user-facing diagnostics
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

> **Why so many rattler crates?**  rattler is split into fine-grained crates so
> you can depend on only the parts you need.  A tool that only needs to solve
> dependencies doesn't have to pull in the HTTP stack.  We use most of them, so
> the list is long.

## The entry point: `src/main.rs`

Let's look at how the project is structured from the top.

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

There is a lot going on here.  Let's take it section by section.

## Rust concept: derive macros

The line `#[derive(Debug, Parser)]` is a **derive macro**.  It automatically
generates an `impl Parser for Cli` block based on the struct's fields and
attributes.  Clap reads the doc comments (`///`) as the help text.

```rust
/// A minimal Lua package manager powered by rattler.
#[derive(Debug, Parser)]
struct Cli { ... }
```

Running `luapkg --help` produces:

```
A minimal Lua package manager powered by rattler.

Usage: luapkg [OPTIONS] <COMMAND>

Commands:
  init     Create a new luapkg.toml in the current directory
  add      Add one or more packages to the manifest and install them
  ...

Options:
  -v, --verbose  Enable verbose logging
  -h, --help     Print help
  -V, --version  Print version
```

All of that text came from the comments in our Rust code.  No hand-written help
strings needed.

## Rust concept: enums for subcommands

```rust
#[derive(Debug, clap::Subcommand)]
enum Command {
    Init(commands::init::Args),
    Add(commands::add::Args),
    ...
}
```

Each variant holds the `Args` struct for that subcommand.  This is idiomatic
Rust: the enum *contains* the data, so after parsing you pattern-match on it and
get the relevant args out.

```rust
match cli.command {
    Command::Init(args) => commands::init::execute(args).await,
    ...
}
```

The compiler ensures you've handled every variant.  If you add a new command and
forget to add a match arm, it's a compile error.

## Rust concept: async and Tokio

Most of our commands do network I/O — fetching package metadata, downloading
archives.  Rust's `async`/`await` syntax lets us write asynchronous code that
*looks* synchronous.

But `async fn main()` doesn't exist in stable Rust.  We need an **async
runtime** to actually drive the futures.  We use **Tokio**:

```rust
fn main() -> miette::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_cpus / 2)
        .max_blocking_threads(num_cpus)
        .enable_all()
        .build()
        .into_diagnostic()?;

    runtime.block_on(async_main())
}
```

`block_on` runs an async future on the current thread until it completes.  The
multi-thread builder spawns a pool of worker threads to handle concurrent tasks —
important when we're downloading multiple packages in parallel.

> **Why `worker_threads(num_cpus / 2)` and `max_blocking_threads(num_cpus)`?**
>
> Tokio has two thread pools:
>
> - **Worker threads** run async tasks (futures).  They should roughly match the
>   number of CPU cores, because futures cooperate — they yield when they would
>   block.
>
> - **Blocking threads** run synchronous code via `spawn_blocking`.  These can
>   block indefinitely without harming the async scheduler.  Package extraction
>   is CPU-bound and synchronous, so it goes here.
>
> Splitting the CPU budget between them avoids one pool starving the other.

## Rust concept: the `?` operator and error handling

```rust
.build()
.into_diagnostic()?
```

The `?` operator is Rust's primary error propagation mechanism.  If the
expression evaluates to `Err(e)`, `?` returns early from the current function
with that error.  If it's `Ok(v)`, `?` unwraps to `v`.

`into_diagnostic()` converts any `Error` type that implements `std::error::Error`
into `miette::Report`, which is our application-level error type.  We'll cover
error handling in depth in Chapter 3.

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
