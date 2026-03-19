# Chapter 5: The `install` Command

The install command is the core of moonshot. It reads the manifest, fetches
repodata (using the same Gateway we set up in [Chapter 4](ch04-search.md)), solves for a
compatible set of package versions, and installs them into a local prefix.

## Design

`shot install` runs the full pipeline: read manifest, discover packages,
solve dependencies, install.

```console
$ shot install
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
  Downloading and extracting packages...
✔ Environment updated in 2.1s
  Activate with:  eval $(shot shell)
```

The command accepts an optional `--prefix` flag to override the install
location. By default, packages are installed to `.env/` relative to the
project root.

## Configuration

### The prefix directory

By convention, `moonshot` puts the environment at `.env/` relative to the
project root, alongside `moonshot.toml`.  This keeps the environment close to the
project and out of the user's global namespace. The user can override it with
`--prefix /path/to/env`.

``` {.rust file=src/commands/mod.rs}
pub mod add;
pub mod build;
pub mod init;
pub mod install;
pub mod run;
pub mod search;
pub mod shell;

use std::path::{Path, PathBuf};

/// Return the path to the conda prefix managed by `moonshot`.
///
/// By convention we store the environment at `.env/` relative to the
/// project root (the directory that contains `moonshot.toml`).  This is similar
/// to how pixi stores its environments in `.pixi/envs/`.
pub fn prefix_dir(project_root: &Path) -> PathBuf {
    project_root.join(".env")
}
```

## Concepts: Solving

As discussed in [Chapter 1](ch01-what-is-a-package-manager.md), conda enforces the "exactly one version per package" constraint. This single rule is what makes solving NP-hard, and it is the reason we need a SAT-based solver instead of a simple graph traversal.

### Why solving is hard

Imagine you ask for two packages:

```toml
[dependencies]
web-server = "*"
json-lib   = "*"
```

And the catalog says:

- `web-server 2.0` depends on `json-lib >=2.0`
- `web-server 1.0` depends on `json-lib >=1.0,<2.0`
- `json-lib 2.1` (latest)
- `json-lib 1.9`

The solver tries `web-server 2.0` + `json-lib 2.1`.  That works.  Done.

But now add another constraint:
```toml
legacy-plugin = "*"  # depends on json-lib <2.0
```

Now `web-server 2.0` is incompatible with `legacy-plugin`.  The solver has to
backtrack and try `web-server 1.0` + `json-lib 1.9` + `legacy-plugin`.

In the general case, dependency solving is equivalent to
[SAT] (Boolean satisfiability), which is NP-hard.  In practice, real package
ecosystems have structure that makes good heuristics very effective.

[SAT]: https://en.wikipedia.org/wiki/Boolean_satisfiability_problem

!!! note "Deep dive"

    For a detailed look at how resolvo's SAT solver works, including clause
    learning, decision heuristics, and the connection to CDCL, see
    [Deep Dive: The resolvo SAT Solver](deep-dive-resolvo.md).

### Virtual packages

Virtual packages model the host system as if it were a package. Instead of special-casing "requires glibc 2.17" as a platform check, the solver treats `__glibc` as a regular dependency that happens to be provided by the OS. This lets package authors express system requirements using the same constraint syntax they use for library dependencies.

The system is probed for things like:

- `__linux` (whether this is a Linux system)
- `__glibc =2.38` (the installed glibc version)
- `__cuda =12.3` (the CUDA toolkit version, if any)
- `__osx =14.4` (macOS version)

Packages can list these as dependencies.  A CUDA-accelerated package might say
`__cuda >=11.0`; the solver will refuse to install it on a machine without CUDA.

!!! note "Deep dive"

    For more on virtual packages and archspec, see
    [Deep Dive: Virtual Packages](deep-dive-virtual-packages.md).

### Locked vs pinned packages

We scan the prefix's `conda-meta/` directory to find out what is already
installed and pass those to the solver as **locked packages**: versions the
solver should prefer to keep if possible.

!!! warning "Why locking matters"

    Without locking, every `shot install` could silently upgrade transitive
    dependencies even when the manifest hasn't changed. That kind of drift is a
    common source of "it worked yesterday" bugs. Locking gives you environmental
    stability: the solver only changes what it must to satisfy new or modified
    constraints.

!!! tip "Locked vs pinned"

    The difference between locked and pinned is important: locked packages are a
    *preference* that the solver may override if constraints demand it, while
    pinned packages are a *hard constraint* that the solver must satisfy or
    report as a conflict.

### Resolver strategy: how conda sorting works

The solver needs a way to choose between `lua 5.4.7` and `lua 5.4.6` when both
satisfy `>=5.4`.  The conda convention is:

1. Prefer **higher versions** of directly-requested packages.
2. Prefer **locked** (currently installed) versions of transitive dependencies.
3. Among unlocked, prefer **higher build numbers** (more recent builds of the
   same version).
4. Prefer packages from channels listed **earlier** in the channel list.

This biases the solver toward fresh versions for things you asked for, while
keeping the rest of your environment stable.  resolvo implements these priorities
through a scoring system, not a separate post-processing step. Without the "prefer locked for transitive deps" rule, adding one new package could cascade upgrades across your entire environment.

## Concepts: Installation

### The package cache

Every package is first extracted into a *central cache* shared across all
environments on the machine (at `~/.rattler/pkgs/`).  The cache key is the
package's content hash, so `lua-5.4.7` is stored exactly once regardless of how
many environments use it. Content-addressed keys (rather than name-plus-version) prevent collisions when the same version is rebuilt with a different build string. Two builds of `lua-5.4.7` with different compiler flags get different hashes and coexist safely in the cache.

### Hard links

!!! info "Why hard-linking is safe"

    Packages in the cache are immutable after extraction. No tool or environment
    modifies them in place. This invariant is what makes hard-linking safe:
    multiple environments can share the same inodes because nobody writes to
    them.

Files are *hard-linked* from the cache into the target prefix.  A hard link is a
second directory entry pointing to the same inode.  The data on disk is stored
once, but it appears in two places.  Removing the link from one location doesn't
affect the other.

This means:

- An environment takes almost no disk space for packages that are already cached.
- Creating a new environment is very fast (linking is cheap).

On filesystems that don't support hard links (some network filesystems, Windows
cross-volume), rattler falls back to copying.

### Transactions

!!! warning "Partial installs"

    A naive package manager that unpacks files one by one can leave an
    environment half-installed if the process is interrupted. Partial installs
    are one of the most common failure modes in package management and often
    require manual cleanup.

The Installer computes a **transaction**, a diff between the currently-installed
state and the desired state, and applies only the changes:

- Install packages not currently present
- Remove packages no longer needed
- Update packages whose version changed

This makes `shot install` idempotent: running it twice with the same manifest
is a no-op.

!!! note "Deep dive"

    For a detailed look at the .conda archive format, inner archives, and
    content-addressed storage, see [Deep Dive: The conda Package Format](deep-dive-package-format.md).

## Implementation

### `src/commands/install.rs`

Here is the full file skeleton, with each section defined as we encounter it:

``` {.rust file=src/commands/install.rs}
<<install-imports>>

<<install-args>>

<<install-from-manifest>>

<<install-execute>>

<<install-private-helpers>>
```

#### Imports

``` {.rust #install-imports}
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseMatchSpecOptions, Platform,
    PrefixRecord, RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::manifest::Manifest;
use crate::progress::with_spinner;
```

#### Args

``` {.rust #install-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
```

#### The `install_from_manifest` function

This function contains all the install logic and is reused by `shot add` ([Chapter 6](ch06-add.md)).

``` {.rust #install-from-manifest}
/// Shared install logic used by both `install` and `add`.
///
/// Takes a fully-parsed `Manifest` and installs (or updates) the environment
/// at `prefix`.  Pulling this out into its own function means `add` can call
/// it after mutating the manifest without duplicating any networking or
/// solving code.
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<()> {
    <<install-parse-specs>>

    <<install-cache-dir>>

    <<install-http-client>>

    <<install-parse-channels>>

    <<install-gateway-builder>>

    <<install-gateway-query>>

    <<install-virtual-packages>>

    <<install-read-installed>>

    <<install-solver-task>>

    <<install-solve>>

    <<install-solve-progress>>

    <<install-installer>>

    <<install-result>>
}
```

#### Parsing specs from the manifest

In `moonshot`, we parse MatchSpecs from the manifest's `[dependencies]` table:

``` {.rust #install-parse-specs}
let channel_config =
    ChannelConfig::default_with_root_dir(env::current_dir().into_diagnostic()?);

let match_spec_opts = ParseMatchSpecOptions::default();
let specs: Vec<MatchSpec> = manifest
    .dependencies
    .iter()
    .map(|(name, version)| {
        let spec_str = if version == "*" {
            name.clone()
        } else {
            format!("{name} {version}")
        };
        MatchSpec::from_str(&spec_str, match_spec_opts)
            .into_diagnostic()
            .with_context(|| format!("parsing spec `{spec_str}`"))
    })
    .collect::<miette::Result<_>>()?;
```

#### Cache directory

We locate the shared rattler cache (see [Chapter 4](ch04-search.md) for background on
content-addressed caching) and ensure it exists on disk:

``` {.rust #install-cache-dir}
let cache_dir = rattler::default_cache_dir()
    .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
rattler_cache::ensure_cache_dir(&cache_dir)
    .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;
```

#### HTTP client

We build a `reqwest` client with `.no_gzip()` (rattler handles repodata
decompression itself) and wrap it with authentication and OCI middleware. See
[Chapter 4](ch04-search.md) for why each middleware layer exists.

``` {.rust #install-http-client}
let raw_client = reqwest::Client::builder()
    .no_gzip() // repodata is already compressed; we handle it ourselves
    .build()
    .expect("failed to build HTTP client");

let client = reqwest_middleware::ClientBuilder::new(raw_client.clone())
    .with_arc(Arc::new(
        AuthenticationMiddleware::from_env_and_defaults()
            .into_diagnostic()
            .context("setting up auth middleware")?,
    ))
    .with(rattler_networking::OciMiddleware::new(raw_client))
    .build();
```

#### Parsing channels

We convert the manifest's channel strings into rattler `Channel` objects. A
`Channel` knows how to construct sub-URLs for different platforms.

``` {.rust #install-parse-channels}
let channels: Vec<Channel> = manifest
    .project
    .channels
    .iter()
    .map(|s| Channel::from_str(s, &channel_config))
    .collect::<Result<_, _>>()
    .into_diagnostic()
    .context("parsing channels")?;
```

#### Building the Gateway

The Gateway is the long-lived object that manages repodata fetching, caching,
and the HTTP client. We configure it to prefer the sharded format when available.

``` {.rust #install-gateway-builder}
let platform = Platform::current();

let gateway = Gateway::builder()
    .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
    .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
    .with_client(client.clone())
    .with_channel_config(rattler_repodata_gateway::ChannelConfig {
        default: SourceConfig {
            sharded_enabled: true, // prefer the fast sharded format
            ..SourceConfig::default()
        },
        per_channel: HashMap::new(),
    })
    .finish();
```

#### Querying the Gateway

With the gateway configured, we fetch the repodata for our requested packages.

``` {.rust #install-gateway-query}
let repo_data: Vec<RepoData> = with_spinner(
    "Fetching repodata",
    gateway
        .query(channels, [platform, Platform::NoArch], specs.clone())
        .recursive(true),
)
.await
.into_diagnostic()
.context("fetching repodata")?;

let total_records: usize = repo_data.iter().map(RepoData::len).sum();
println!(
    "  {} repodata records loaded",
    console::style(total_records).cyan()
);
```

`gateway.query(...)` builds a `Query` object.  `.recursive(true)` makes the
critical difference: instead of only fetching records for the directly-requested
packages, it recursively fetches records for their dependencies, and their
dependencies' dependencies, until the transitive closure is complete.

We pass two platforms: `Platform::current()` (e.g., `linux-64`) and
`Platform::NoArch`.  NoArch covers pure-Lua packages that work on every
platform.

The query returns a `Vec<RepoData>`, one entry per channel/platform combination.
Each `RepoData` is a set of `RepoDataRecord`s, enriched package descriptors
that include the download URL alongside the metadata.

#### Detecting virtual packages

Before calling the solver, we detect what the host system provides. These
synthetic packages let the solver reject incompatible packages early.

``` {.rust #install-virtual-packages}
let virtual_packages: Vec<GenericVirtualPackage> =
    rattler_virtual_packages::VirtualPackage::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )
    .into_diagnostic()
    .context("detecting virtual packages")?
    .into_iter()
    .map(|v| v.into())
    .collect();
```

`GenericVirtualPackage` is a simpler wrapper around `VirtualPackage` that
stores the name and version as strings, which is what the solver expects.

#### Reading installed packages

We scan the prefix's `conda-meta/` directory to find out what is already
installed. These records become locked packages in the solver task.

``` {.rust #install-read-installed}
let installed_packages =
    PrefixRecord::collect_from_prefix::<PrefixRecord>(&prefix).into_diagnostic()?;
```

`PrefixRecord` is rattler's representation of a package that's already installed.
When rattler installs a package, it writes a JSON file to
`<prefix>/conda-meta/<name>-<version>-<build>.json` describing what was
installed.  `collect_from_prefix` reads all of those files.

#### Building the solver task

We assemble the installed packages, virtual packages, specs, and repodata into
a single `SolverTask`. The installed packages are passed as locked packages so
the solver prefers to keep them.

``` {.rust #install-solver-task}
let locked = installed_packages
    .iter()
    .map(|r| r.repodata_record.clone())
    .collect::<Vec<_>>();

let solver_task = SolverTask {
    locked_packages: locked,
    virtual_packages,
    specs: specs.clone(),
    ..SolverTask::from_iter(&repo_data)
};
```

`SolverTask::from_iter(&repo_data)` builds the task's `available_packages` field
from our repodata.  We override the remaining fields with our specs, locked
packages, and virtual packages.

The complete `SolverTask` contains:

| Field | Description |
|---|---|
| `available_packages` | All packages the solver may choose from |
| `specs` | What the user requested |
| `locked_packages` | Currently installed (prefer to keep) |
| `virtual_packages` | Host system capabilities |
| `pinned_packages` | Packages that must stay at a specific version |

#### Running the solver

rattler ships two solver backends: `resolvo` (pure Rust, the default, used by pixi) and `libsolv_c` (a C binding to [libsolv], used by older conda tooling).  We use resolvo throughout this book.

``` {.rust #install-solve}
let start_solve = Instant::now();
let solution: Vec<RepoDataRecord> =
    with_spinner_sync("Solving", || resolvo::Solver.solve(solver_task))
        .into_diagnostic()
        .context("solving dependencies")?
        .records;
```

`resolvo::Solver.solve(solver_task)` is synchronous and CPU-bound.  We run it in
`with_spinner_sync`, a version of our spinner helper that works with synchronous
closures.

#### Printing solve progress

After solving, we report how many packages were selected and how long it took.

``` {.rust #install-solve-progress}
println!(
    "  Solved {} packages in {:.1}s",
    console::style(solution.len()).cyan(),
    start_solve.elapsed().as_secs_f64()
);
```

#### The Installer

We configure the installer with a builder and call `.install()` to apply the transaction.

``` {.rust #install-installer}
let start_install = Instant::now();
let result = Installer::new()
    .with_download_client(client)
    .with_target_platform(platform)
    .with_installed_packages(installed_packages)
    .with_execute_link_scripts(true)
    .with_requested_specs(specs)
    .with_reporter(IndicatifReporter::builder().finish())
    .install(&prefix, solution)
    .await
    .into_diagnostic()
    .context("installing packages")?;
```

`IndicatifReporter` is a rattler-provided reporter that shows per-package
progress bars during download and extraction. You can implement your own
reporter if you want custom progress display; it's a trait, not a concrete type.

Setting `with_execute_link_scripts(true)` tells the installer to run conda's
**link scripts** after installation. These are scripts in
`<prefix>/etc/conda/activate.d/` that some packages use to set up post-install
configuration (updating `LUA_PATH`, for example).

The installer needs to know which packages were *directly* requested (as opposed
to installed as transitive dependencies) via `with_requested_specs`. It records
this in the `conda-meta/*.json` files so that future updates can correctly
distinguish "user wants this" from "installed because something else needed it".

!!! info "Tracking direct vs transitive"

    This distinction drives automatic cleanup: when a direct dependency is
    removed, the installer can garbage-collect its transitive dependencies that
    nothing else needs. Both npm and pip added this tracking late in their
    development, and the lack of it caused years of accumulated orphan packages
    in user environments.

#### Reading the result

Once installation finishes, we check whether the transaction changed anything.
If all operations are empty, the environment was already up to date.

``` {.rust #install-result}
if result.transaction.operations.is_empty() {
    println!(
        "{} Environment already up to date",
        console::style("✔").green()
    );
} else {
    println!(
        "{} Environment updated in {:.1}s",
        console::style("✔").green(),
        start_install.elapsed().as_secs_f64()
    );
    println!("  Activate with:  eval $(shot shell)");
}

Ok(())
```

`result.transaction.operations` is a list of what the installer did.
If it's empty, nothing changed.

#### The execute function

The `execute` function is a thin entry point that reads the manifest and calls
`install_from_manifest`:

``` {.rust #install-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (_, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    install_from_manifest(&manifest, prefix).await
}
```

#### Private helpers

A thin wrapper that forwards to the progress module's sync spinner. This avoids
importing the progress module in every call site.

``` {.rust #install-private-helpers}
fn with_spinner_sync<T, F: FnOnce() -> T>(msg: &'static str, f: F) -> T {
    crate::progress::with_spinner_sync(msg, f)
}
```

### The sync spinner

The solver is synchronous, so it needs a sync version of the spinner. This lives
in `src/progress.rs`:

``` {.rust #with-spinner-sync}
pub fn with_spinner_sync<T, F: FnOnce() -> T>(msg: impl Into<Cow<'static, str>>, f: F) -> T {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = f();
    pb.finish_and_clear();
    result
}
```

## Running `shot install`

```console
$ shot install
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
  Downloading and extracting packages...
✔ Environment updated in 2.1s
  Activate with:  eval $(shot shell)
```

### What gets installed where

After `shot install`, the prefix looks like this:

```text
.env/
├── bin/
│   ├── lua             ← the Lua interpreter
│   └── luarocks        ← LuaRocks (if installed)
├── lib/
│   ├── liblua.so.5.4
│   └── ...
├── share/
│   └── lua/5.4/
│       └── ...         ← pure-Lua libraries
└── conda-meta/
    ├── lua-5.4.7-h5eee18b_0.json
    └── ...             ← one file per installed package
```

The `conda-meta/` directory is rattler's installation database.  Each JSON file
records the package name, version, build, all installed files, and their hashes.

## Summary

- The install command runs the full pipeline: discovery, solving, installation.
- The `Gateway` fetches repodata with `.recursive(true)` to get all transitive
  dependencies.
- Virtual packages represent host-system capabilities (glibc, CUDA, etc.).
- The `SolverTask` bundles available packages, specs, locked packages, and
  virtual packages.
- The `Installer` computes a transaction (diff) and applies only the changes.
- Files are hard-linked from the central cache into the prefix.
- `install_from_manifest` is shared with the `add` command (next chapter).

In the next chapter we implement `shot add`, a thin wrapper that updates the
manifest and then installs.

[libsolv]: https://github.com/openSUSE/libsolv
