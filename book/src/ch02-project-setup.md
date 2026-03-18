# Chapter 2: Setting Up the Project

In this chapter we create the Rust project, add our dependencies, and build the
CLI skeleton with [Clap].  By the end of the chapter `luapkg --help` works, even
though none of the commands do anything yet.

## Creating the project

```console
cargo new luapkg
cd luapkg
```

## Dependencies

Open `Cargo.toml` and add the following.  We'll explain each crate as we use it.

``` {.toml file=Cargo.toml}
[package]
name = "luapkg"
version = "0.1.0"
edition = "2021"
description = "A minimal Lua package manager built on rattler"

[[bin]]
name = "luapkg"
path = "src/main.rs"

[features]
default = ["rustls-tls"]
rustls-tls = [
    "reqwest/rustls-tls",
    "reqwest/rustls-tls-native-roots",
    "rattler/rustls-tls",
    "rattler_cache/rustls-tls",
    "rattler_networking/rustls-tls",
    "rattler_repodata_gateway/rustls-tls",
]

[dependencies]
# Core rattler crates
rattler = { version = "0.40.0", default-features = false, features = ["indicatif"] }
rattler_cache = { version = "0.6.15", default-features = false }
rattler_conda_types = { version = "0.44.0", default-features = false }
rattler_digest = { version = "1.2.2", default-features = false }
rattler_networking = { version = "0.26.3", default-features = false, features = ["system-integration"] }
rattler_package_streaming = { version = "0.24.3", default-features = false }
rattler_repodata_gateway = { version = "0.27.0", default-features = false, features = ["gateway"] }
rattler_shell = { version = "0.26.3", default-features = false }
rattler_solve = { version = "5.0.0", default-features = false, features = ["resolvo"] }
rattler_index = { version = "0.27.17", default-features = false }
rattler_virtual_packages = { version = "2.3.12", default-features = false }

# Async runtime
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process"] }

# CLI argument parsing
clap = { version = "4", features = ["derive", "color", "suggestions"] }

# Error handling — miette gives beautiful terminal diagnostics
miette = { version = "7", features = ["fancy"] }
thiserror = "2"

# Serialization — manifest I/O
serde = { version = "1", features = ["derive"] }
toml = "0.9"

# HTTP client (versions must match what rattler expects)
reqwest = { version = "0.12", default-features = false, features = ["stream"] }
reqwest-middleware = "0.4"

# Progress bars
indicatif = "0.18"

# Console formatting
console = "0.16"

# Timestamps for package metadata
chrono = { version = "0.4", default-features = false, features = ["std", "clock"] }

# SHA-256 for paths.json integrity data
sha2 = "0.10"

# JSON serialization used directly in build command
serde_json = "1"

# Temporary directory for build workspace
tempfile = "3"

# Directory walking for collecting build outputs
walkdir = "2"

# Logging / tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

The rattler crates (`rattler`, `rattler_solve`, `rattler_shell`, etc.) implement the conda specification. The rest of the dependency list is general-purpose infrastructure: `clap` for CLI parsing, `tokio` for async I/O, `reqwest` for HTTP, and so on.

!!! question "Why so many rattler crates?"

    rattler is split into fine-grained crates so you can depend on only the
    parts you need.  A tool that only needs to solve dependencies doesn't have
    to pull in the HTTP stack.  We use most of them, so the list is long.

Package managers surface errors from many sources (network, filesystem, solver conflicts, malformed metadata), so [miette] with `features = ["fancy"]` is worth pulling in early. It renders structured diagnostics with source spans, which makes dependency conflicts and parse errors much easier to read than a plain error string.

Notice that `reqwest` is declared with `features = ["stream"]` for streaming downloads. TLS is handled at the crate level through the `[features]` section, where the `rustls-tls` feature propagates `reqwest/rustls-tls` and `reqwest/rustls-tls-native-roots` (along with matching features for the rattler crates). This selects the pure-Rust TLS implementation without linking against the system's OpenSSL.

## The entry point: `src/main.rs`

Here is how the project is structured:

```text
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

The full `main.rs` is assembled from four named sections:

``` {.rust file=src/main.rs}
<<main-imports>>

<<main-cli-struct>>

<<main-fn>>

<<main-async>>
```

Let's take it section by section.

### Imports and module declarations

The imports pull in Clap for argument parsing, miette for error reporting, and
the tracing filter types for log-level control. The `mod` declarations make the
rest of our crate visible.

``` {.rust #main-imports}
use clap::Parser;
use miette::IntoDiagnostic;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

mod commands;
mod manifest;
mod progress;
mod recipe;
```

### The CLI struct and subcommands

Clap's derive macros turn these struct and enum definitions into a full CLI
parser. Each variant of `Command` maps to a subcommand and carries its own
argument struct (defined in the corresponding module).

``` {.rust #main-cli-struct}
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

    /// Search for packages in a channel.
    Search(commands::search::Args),

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
```

### The synchronous entry point

`fn main` builds a [Tokio] runtime and blocks on `async_main`. We configure two
thread pools: `worker_threads` for async work (HTTP requests, futures polling)
and `max_blocking_threads` for synchronous operations that would stall the async
scheduler (file I/O, archive extraction, solver runs).

``` {.rust #main-fn}
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
```

!!! info "Thread pool sizing"

    The runtime configuration splits available CPU cores between two pools.
    `worker_threads` handles async work (HTTP requests, futures polling), while
    `max_blocking_threads` handles synchronous operations that would stall the
    async scheduler (file I/O, archive extraction, solver runs). Giving the
    blocking pool more threads prevents slow disk operations from starving
    network requests.

### The async entry point

`async_main` parses the CLI arguments, sets up logging, and dispatches to the
right subcommand handler. We route all log output to stderr, leaving stdout
clean for machine-readable output (like `luapkg shell`, which prints a shell
script). The `--verbose` flag raises the log level to `DEBUG`; users can also
set `RUST_LOG=debug` for more control.

``` {.rust #main-async}
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
        Command::Search(args)  => commands::search::execute(args).await,
        Command::Add(args)     => commands::add::execute(args).await,
        Command::Install(args) => commands::install::execute(args).await,
        Command::Shell(args)   => commands::shell::execute(args),
        Command::Run(args)     => commands::run::execute(args).await,
        Command::Build(args)   => commands::build::execute(args).await,
    }
}
```

## Summary

- We set up a Rust project with a clean module tree.
- Clap's derive macros turn struct definitions into a full CLI parser.
- Tokio provides the async runtime; we configure two thread pools.

[Clap]: https://docs.rs/clap
[miette]: https://docs.rs/miette
[Tokio]: https://tokio.rs

In the next chapter we implement the simplest command: `luapkg init`, which
writes the project manifest.
