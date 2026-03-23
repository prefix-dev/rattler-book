# Chapter 6: The `lock` Command

The manifest declares what you want; now we need to figure out exactly
which packages satisfy those requirements. The lock command resolves
your dependencies into exact packages and records the solution. Later
commands (like `shot install`) consume the lock file instead of
re-solving from scratch.

## Design

`shot lock` resolves dependencies and writes the lock file:

```console
$ shot lock
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
✔ Wrote moonshot.lock (5 packages)
```

If the manifest hasn't changed since the last solve, the command returns
immediately:

```console
$ shot lock
✔ Lock is already up to date
```

The `--force` flag re-solves even when the lock is fresh:

```console
$ shot lock --force
⠋ Fetching repodata
  ...
✔ Wrote moonshot.lock (5 packages)
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

Virtual packages model the host system as if it were a regular package. Instead of special-casing "requires glibc 2.17" as a platform check, the solver treats `__glibc` as a regular dependency that happens to be provided by the OS. This lets package authors express system requirements using the same constraint syntax they use for library dependencies.

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

When an existing lock file is present (even if stale), we read its records
and pass them to the solver as **locked packages**: versions the solver
should prefer to keep if possible. This gives stable upgrades; re-solving
only changes what the new constraints require.

!!! warning "Why locking matters"

    Without locking, every resolve could silently upgrade transitive
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

## Concepts: Lock Files

### Why lock files

Without a lock file, the solver picks the best solution *at the time you run
it*. A lock file records the *exact* solution: every package name, version,
build string, and download URL. Replaying the lock gives you the same
environment every time.

Every serious package manager converges on this pattern:

| Tool | Lock file |
|---|---|
| Cargo | `Cargo.lock` |
| npm | `package-lock.json` |
| pip | `requirements.txt` (manual) / `uv.lock` |
| pixi | `pixi.lock` |
| moonshot | `moonshot.lock` |

### Two-phase model

The lock file splits our install into two phases:

1. **Resolve** (slow): fetch repodata, run the SAT solver, write the lock.
2. **Install from lock** (fast): read exact packages from the lock, download
   and link them. No solver, no repodata fetch beyond what's cached.

This is the same split Cargo uses: `cargo update` resolves, `cargo build`
installs from the lock.

### Freshness detection

How do we know when to re-solve? We compare file modification times:

- If `moonshot.lock` is **newer** than `moonshot.toml`, the lock is fresh.
  The manifest hasn't changed since the last solve.
- If `moonshot.toml` is **newer** (or the lock doesn't exist), we re-solve.

### The `rattler_lock` format

`moonshot.lock` is a YAML file following the `rattler_lock` crate's format (the
same format pixi uses for `pixi.lock`). A simplified example:

```yaml
version: 6
environments:
  default:
    channels:
      - url: https://conda.anaconda.org/conda-forge/
    packages:
      osx-arm64:
        - conda: https://conda.anaconda.org/conda-forge/osx-arm64/lua-5.4.7-h5eee18b_0.conda
          ...
      noarch:
        - conda: https://conda.anaconda.org/conda-forge/noarch/luafilesystem-1.8.0-lua_0.conda
          ...
```

Each entry records the exact URL, SHA-256 hash, and full dependency metadata.
The `rattler_lock` crate handles serialization and deserialization.

## Implementation

### `src/resolve.rs`

The resolve module is a shared helper that we'll reuse in both the lock and
install commands. It handles the full resolve pipeline: parse specs from the
manifest, set up an HTTP client and gateway (the same pattern we built in
[Chapter 4](ch04-search.md)'s search command), fetch repodata recursively,
detect virtual packages, and run the solver.

``` {.rust file=src/resolve.rs}
<<resolve-imports>>

<<resolve-read-locked>>

<<resolve-fn>>

<<resolve-helpers>>
```

#### Imports

``` {.rust #resolve-imports}
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use miette::{Context, IntoDiagnostic};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseMatchSpecOptions, Platform,
    RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::lock::read_lock_file;
use crate::manifest::Manifest;
use crate::progress::with_spinner;
```

#### Reading existing locked packages

Before resolving, both `shot lock` and `shot install` read any existing lock
file to extract locked packages. If the file is missing or unreadable, an
empty vector is returned so the solver starts fresh.

``` {.rust #resolve-read-locked}
/// Read existing locked packages from the lock file, if it exists.
///
/// Returns the records from the lock or an empty vector if the file
/// is missing or unreadable. These records can be passed to the solver
/// as `locked_packages` for stable upgrades.
pub fn read_locked_packages(
    lock_path: &std::path::Path,
    platform: Platform,
) -> Vec<RepoDataRecord> {
    if lock_path.exists() {
        read_lock_file(lock_path, platform).unwrap_or_default()
    } else {
        Vec::new()
    }
}
```

#### The resolve function

`resolve_from_manifest` runs the full resolve pipeline and returns the
solution, the parsed channels, and the current platform. Callers use the
channels and platform to write the lock file.

``` {.rust #resolve-fn}
/// Resolve dependencies from a manifest.
///
/// This is the shared resolve pipeline: parse specs, set up the HTTP
/// client and gateway, fetch repodata, detect virtual packages, and
/// run the solver. Both `shot lock` and `shot install` call this.
pub async fn resolve_from_manifest(
    manifest: &Manifest,
    locked_packages: Vec<RepoDataRecord>,
) -> miette::Result<(Vec<RepoDataRecord>, Vec<Channel>, Platform)> {
    <<parse-specs>>
    <<setup-client>>
    <<fetch-repodata>>
    <<run-solver>>
}
```

##### Parsing match specs

We convert each `(name, version)` pair from the manifest into a `MatchSpec`,
the same way the search command does in [Chapter 4](ch04-search.md).

``` {.rust #parse-specs}
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

##### Setting up the HTTP client

The HTTP client and authentication middleware follow the same pattern from
[Chapter 4](ch04-search.md). See that chapter for a detailed walkthrough.

``` {.rust #setup-client}
let cache_dir = rattler::default_cache_dir()
    .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
rattler_cache::ensure_cache_dir(&cache_dir)
    .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;

let raw_client = reqwest::Client::builder()
    .no_gzip()
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

##### Fetching repodata

Next we parse channels, set up the Gateway, and query repodata. The key
difference from the search command is `.recursive(true)`, because we need transitive
dependencies, not just direct matches.

``` {.rust #fetch-repodata}
let channels: Vec<Channel> = manifest
    .project
    .channels
    .iter()
    .map(|s| Channel::from_str(s, &channel_config))
    .collect::<Result<_, _>>()
    .into_diagnostic()
    .context("parsing channels")?;

let platform = Platform::current();

let gateway = Gateway::builder()
    .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
    .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
    .with_client(client)
    .with_channel_config(rattler_repodata_gateway::ChannelConfig {
        default: SourceConfig {
            sharded_enabled: true,
            ..SourceConfig::default()
        },
        per_channel: HashMap::new(),
    })
    .finish();

let repo_data: Vec<RepoData> = with_spinner(
    "Fetching repodata",
    gateway
        .query(
            channels.clone(),
            [platform, Platform::NoArch],
            specs.clone(),
        )
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

##### Running the solver

Finally we detect virtual packages (like `__linux` or `__osx`), build a
`SolverTask`, and run the solver. `locked_packages` allows callers to pass
records from an existing (stale) lock file. The solver treats them as
preferences, not hard constraints. On the first run with no lock, callers pass
an empty vector.

``` {.rust #run-solver}
let virtual_packages: Vec<GenericVirtualPackage> =
    rattler_virtual_packages::VirtualPackage::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )
    .into_diagnostic()
    .context("detecting virtual packages")?
    .into_iter()
    .map(|v| v.into())
    .collect();

let solver_task = SolverTask {
    locked_packages,
    virtual_packages,
    specs,
    ..SolverTask::from_iter(&repo_data)
};

let start_solve = Instant::now();
let solution = with_spinner_sync("Solving", || resolvo::Solver.solve(solver_task))
    .into_diagnostic()
    .context("solving dependencies")?
    .records;

println!(
    "  Solved {} packages in {:.1}s",
    console::style(solution.len()).cyan(),
    start_solve.elapsed().as_secs_f64()
);

Ok((solution, channels, platform))
```

#### Sync spinner helper

``` {.rust #resolve-helpers}
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

### `src/lock.rs`

Let's build the utility module that provides the building blocks: a freshness
check, a reader, and a writer. The lock command will orchestrate them.

``` {.rust file=src/lock.rs}
#![allow(dead_code)]
<<lock-imports>>

<<lock-filename>>

<<lock-is-fresh>>

<<lock-read>>

<<lock-write>>

<<lock-tests>>
```

#### Imports

``` {.rust #lock-imports}
use std::path::Path;

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, Platform, RepoDataRecord};
use rattler_lock::LockFile;
```

#### The lock filename

``` {.rust #lock-filename}
/// The name of the lock file written alongside `moonshot.toml`.
pub const LOCK_FILENAME: &str = "moonshot.lock";
```

#### Freshness check

We compare modification times to decide whether our lock is still valid.

``` {.rust #lock-is-fresh}
/// Returns `true` when the lock file exists and is newer than the manifest.
pub fn is_lock_fresh(lock_path: &Path, manifest_path: &Path) -> bool {
    let (Ok(lock_meta), Ok(manifest_meta)) = (
        std::fs::metadata(lock_path),
        std::fs::metadata(manifest_path),
    ) else {
        return false;
    };
    let (Ok(lock_mtime), Ok(manifest_mtime)) = (lock_meta.modified(), manifest_meta.modified())
    else {
        return false;
    };
    lock_mtime >= manifest_mtime
}
```

If either file is missing or the OS doesn't support modification times, we
conservatively return `false` (re-solve).

#### Reading a lock file

``` {.rust #lock-read}
/// Read a lock file and extract the conda records for the given platform.
///
/// Returns the exact packages that were solved last time, ready to be
/// handed to the `Installer`.
pub fn read_lock_file(lock_path: &Path, platform: Platform) -> miette::Result<Vec<RepoDataRecord>> {
    let lock_file = LockFile::from_path(lock_path)
        .into_diagnostic()
        .context("reading lock file")?;

    let env = lock_file
        .default_environment()
        .ok_or_else(|| miette::miette!("lock file has no default environment"))?;

    let records = env
        .conda_repodata_records(platform)
        .into_diagnostic()
        .context("extracting conda records from lock file")?
        .unwrap_or_default();

    Ok(records)
}
```

`LockFile::from_path` parses the YAML.  `default_environment()` returns the
`"default"` environment (the only one moonshot uses).
`conda_repodata_records` converts the locked packages back into
`RepoDataRecord`s that the `Installer` understands.

#### Writing a lock file

``` {.rust #lock-write}
/// Write a lock file containing the solved packages.
///
/// The lock captures the exact solution so that future installs can skip
/// the solver when the manifest hasn't changed.
pub fn write_lock_file(
    lock_path: &Path,
    channels: &[Channel],
    platform: Platform,
    solution: &[RepoDataRecord],
) -> miette::Result<()> {
    let lock_channels: Vec<rattler_lock::Channel> = channels
        .iter()
        .map(|ch| rattler_lock::Channel {
            url: ch.base_url.to_string(),
            used_env_vars: vec![],
        })
        .collect();

    let mut builder = LockFile::builder();
    builder.set_channels("default", lock_channels);

    for record in solution {
        let conda_pkg: rattler_lock::CondaPackageData = record.clone().into();
        builder.add_conda_package("default", platform, conda_pkg);
    }

    let lock_file = builder.finish();
    lock_file
        .to_path(lock_path)
        .into_diagnostic()
        .context("writing lock file")?;

    Ok(())
}
```

`LockFileBuilder` accumulates packages and channels, deduplicating by content
hash. `finish()` produces a `LockFile` that `to_path` serializes to YAML.

The conversion `record.clone().into()` turns a `RepoDataRecord` into
`rattler_lock::CondaPackageData`, preserving the download URL, hash, and full
dependency metadata.

### `src/commands/lock.rs`

With the resolve logic in `src/resolve.rs`, our lock command ends up quite
small. It checks freshness, calls the resolver, and writes the result.

``` {.rust file=src/commands/lock.rs}
<<lock-cmd-imports>>

<<lock-cmd-args>>

<<lock-cmd-execute>>
```

#### Imports

``` {.rust #lock-cmd-imports}
use std::env;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;

use crate::lock::{is_lock_fresh, write_lock_file, LOCK_FILENAME};
use crate::manifest::Manifest;
use crate::resolve::{read_locked_packages, resolve_from_manifest};
```

#### Args

``` {.rust #lock-cmd-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Force re-solving even if the lock is up to date.
    #[clap(long)]
    pub force: bool,
}
```

#### The execute function

Our execute function has three phases: check freshness, resolve, and write the lock.

``` {.rust #lock-cmd-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;
    let lock_path = cwd.join(LOCK_FILENAME);

    if !args.force && is_lock_fresh(&lock_path, &manifest_path) {
        println!("{} Lock is already up to date", console::style("✔").green());
        return Ok(());
    }

    let platform = Platform::current();
    let existing = read_locked_packages(&lock_path, platform);
    let (solution, channels, platform) = resolve_from_manifest(&manifest, existing).await?;

    write_lock_file(&lock_path, &channels, platform, &solution)?;

    println!(
        "{} Wrote {} ({} packages)",
        console::style("✔").green(),
        LOCK_FILENAME,
        console::style(solution.len()).cyan()
    );

    Ok(())
}
```

The freshness check exits early when the lock is newer than the manifest.
Otherwise we read any existing lock records for the solver's `locked_packages`
preference and call `resolve_from_manifest` to run the full pipeline.

## Testing

Let's add unit tests in `src/lock.rs` to verify that our write/read roundtrip
and the freshness check work correctly.

``` {.rust #lock-tests}
#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::{
        package::CondaArchiveIdentifier, Channel, ChannelConfig, PackageName, PackageRecord,
        Platform, RepoDataRecord,
    };
    use std::str::FromStr;

    /// Build a minimal `RepoDataRecord` for testing.
    fn dummy_record(name: &str, version: &str) -> RepoDataRecord {
        let channel_config =
            ChannelConfig::default_with_root_dir(std::env::current_dir().expect("cwd"));
        let channel = Channel::from_str("conda-forge", &channel_config).unwrap();
        let mut record = PackageRecord::new(
            PackageName::from_str(name).unwrap(),
            rattler_conda_types::VersionWithSource::from_str(version).unwrap(),
            format!("h0_0"),
        );
        record.subdir = Platform::current().to_string();

        RepoDataRecord {
            package_record: record,
            url: format!(
                "https://conda.anaconda.org/conda-forge/{}/{name}-{version}-h0_0.conda",
                Platform::current()
            )
            .parse()
            .unwrap(),
            channel: Some(channel.name().to_string()),
            identifier: CondaArchiveIdentifier::from_str(&format!("{name}-{version}-h0_0.conda"))
                .unwrap()
                .into(),
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(LOCK_FILENAME);
        let channel_config =
            ChannelConfig::default_with_root_dir(std::env::current_dir().expect("cwd"));
        let channels = vec![Channel::from_str("conda-forge", &channel_config).unwrap()];
        let platform = Platform::current();
        let solution = vec![dummy_record("lua", "5.4.7")];

        write_lock_file(&lock_path, &channels, platform, &solution).unwrap();
        assert!(lock_path.exists());

        let records = read_lock_file(&lock_path, platform).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].package_record.name,
            PackageName::from_str("lua").unwrap()
        );
    }

    #[test]
    fn freshness_check() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("moonshot.toml");
        let lock_path = dir.path().join(LOCK_FILENAME);

        // Neither file exists → stale.
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Only manifest exists → stale.
        std::fs::write(&manifest_path, "").unwrap();
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Lock written after manifest → fresh.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&lock_path, "").unwrap();
        assert!(is_lock_fresh(&lock_path, &manifest_path));

        // Manifest touched after lock → stale.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&manifest_path, "changed").unwrap();
        assert!(!is_lock_fresh(&lock_path, &manifest_path));
    }
}
```

Run the tests with `cargo test`:

```console
$ cargo test
running 2 tests
test lock::tests::freshness_check ... ok
test lock::tests::write_then_read_roundtrip ... ok

test result: ok. 2 passed; 0 filtered out
```

## Running `shot lock`

```console
$ shot lock
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
✔ Wrote moonshot.lock (5 packages)
```

Running it again without changing the manifest:

```console
$ shot lock
✔ Lock is already up to date
```

## Summary

- `shot lock` resolves dependencies and writes `moonshot.lock`.
- If the lock is already fresh, the command exits immediately.
- `resolve_from_manifest` is a shared helper that handles the full resolve
  pipeline: specs, HTTP client, gateway, repodata, virtual packages, solver.
- `is_lock_fresh` compares modification times to decide whether to re-solve.
- `read_lock_file` extracts `RepoDataRecord`s from the lock via `rattler_lock`.
- `write_lock_file` builds a `LockFile` from the solver output and writes YAML.

In the next chapter we implement `shot install`, which uses the lock file
as its source of truth for installation.
