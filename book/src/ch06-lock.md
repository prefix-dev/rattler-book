# Chapter 6: The `lock` Command

<span class="newthought">The manifest declares</span> what you want; now we need to figure out exactly
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

Dependency solving is one of those problems that seems trivial until you hit a real conflict. We previously worked on [rip](https://github.com/prefix-dev/rip), a project that would solve PyPI dependencies directly, and we found weird backtracking behaviors and certain combinations of decisions made by library authors that cause really long solve times. The combination of `boto3` and `urllib3` being a [notorious one](https://github.com/prefix-dev/rip/issues/191) in the Python world.

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
[SAT] (Boolean satisfiability), which is NP-hard. SAT is actually the "OG" NP problem via the [Cook-Levin theorem](https://en.wikipedia.org/wiki/Cook%E2%80%93Levin_theorem). In practice, real package
ecosystems have structure that makes good heuristics very effective.

[SAT]: https://en.wikipedia.org/wiki/Boolean_satisfiability_problem

/// margin-note
For a detailed look at how resolvo's SAT solver works, including clause
learning, decision heuristics, and the connection to CDCL, see
[Deep Dive: The resolvo SAT Solver](deep-dive-resolvo.md).
///

### Virtual packages

Virtual packages model the host system as if it were a regular package. Instead of special-casing "requires glibc 2.17" as a platform check, the solver treats `__glibc` as a regular dependency that happens to be provided by the OS. This lets package authors express system requirements using the same constraint syntax they use for library dependencies. The [rattler_virtual_packages] crate detects what the current system provides.

The system is probed for things like:

- `__linux` (whether this is a Linux system)
- `__glibc =2.38` (the installed glibc version)
- `__cuda =12.3` (the CUDA toolkit version, if any)
- `__osx =14.4` (macOS version)

Packages can list these as dependencies.  A CUDA-accelerated package might say
`__cuda >=11.0`; the solver will refuse to install it on a machine without CUDA.

/// margin-note
For more on virtual packages and archspec, see
[Deep Dive: Virtual Packages](deep-dive-virtual-packages.md).
///

### Locked vs pinned packages

When an existing lock file is present (even if stale), we read its records
and pass them to the solver as **locked packages**: versions the solver
should prefer to keep if possible. This gives stable upgrades; re-solving
only changes what the new constraints require.

/// margin-note
Without locking, every resolve could silently upgrade transitive
dependencies even when the manifest hasn't changed. While working in robotics I had situations where an install could work on my machine but the moment a co-worker ran the same installation the program failed because newer apt versions were pulled in. Locking eliminates this entire class of bug.
///

/// margin-note
The difference between locked and pinned is important: locked packages are a
*preference* that the solver may override if constraints demand it, while
pinned packages are a *hard constraint* that the solver must satisfy or
report as a conflict.
///

### Resolver strategy: how conda sorting works

The solver needs a way to choose between `lua 5.4.7` and `lua 5.4.6` when both
satisfy `>=5.4`.  The conda convention is:

1. Prefer **higher versions** of directly-requested packages.
2. Prefer **locked** (currently installed) versions of transitive dependencies.
3. Among unlocked, prefer **higher build numbers** (more recent builds of the
   same version).
4. Prefer packages from channels listed **earlier** in the channel list.

This biases the solver toward fresh versions for things you asked for, while
keeping the rest of your environment stable.  [resolvo] implements these priorities
through a scoring system, not a separate post-processing step. Without the "prefer locked for transitive deps" rule, adding one new package could cascade upgrades across your entire environment.

## Concepts: Lock Files

### Why lock files

Without a lock file, the solver picks the best solution *at the time you run
it*. A lock file records the *exact* solution: every package name, version,
build string, and download URL. Replaying the lock gives you the same
environment every time. We are pretty bullish on locking and it was something we spent a lot of time on with pixi to get right; we are already at the 7th version of our lock file format.

Every serious package manager converges on this pattern:

| Tool | Lock file |
|---|---|
| Cargo | `Cargo.lock` |
| npm | `package-lock.json` |
| pip | `requirements.txt` (manual) / `uv.lock` |
| [pixi] | `pixi.lock` |
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

`moonshot.lock` is a YAML file following the [rattler_lock] crate's format (the
same format [pixi] uses for `pixi.lock`). A simplified example:

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
The [rattler_lock] crate handles serialization and deserialization.

## Implementation

The resolution logic that was previously in a standalone `src/resolve.rs` now
lives in the `Session` struct, which bundles the project, HTTP client, and
repodata gateway into a single object. We introduce it later in this chapter.

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
<<lock-read-locked>>
<<lock-write>>
<<lock-tests>>
```

We start with the file-operation and lock-format imports:

``` {.rust #lock-imports}
use std::path::Path;

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, Platform, RepoDataRecord};
use rattler_lock::LockFile;
```

A single constant keeps the lock filename consistent:

``` {.rust #lock-filename}
/// The name of the lock file written alongside `moonshot.toml`.
pub const LOCK_FILENAME: &str = "moonshot.lock";
```

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

Reading a lock file extracts the conda records for the current platform:

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

Before resolving, both `shot lock` and `shot install` read any existing lock
file to extract locked packages. If the file is missing or unreadable, an
empty vector is returned so the solver starts fresh.

``` {.rust #lock-read-locked}
/// Read existing locked packages from the lock file, if it exists.
///
/// Returns the records from the lock or an empty vector if the file
/// is missing or unreadable.
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

Finally, writing the lock file serializes the solved packages to YAML:

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
```

With the channels converted, we feed each solved record into a `LockFileBuilder`.
The builder deduplicates packages by content hash, so repeated solves produce
identical lock files.

``` {.rust #lock-write}
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

### Extending `Project` with lock helpers

The `Project` struct from [Chapter 5](ch05-add.md) knows where the
manifest lives but has no concept of locking yet. We add two small methods:
`lock_path` returns the expected lock file location, and `is_lock_fresh`
delegates to the freshness check we just wrote.

``` {.rust file=src/project.rs}
<<project-lock-imports>>
<<project-lock-methods>>
```

``` {.rust #project-lock-imports}
use crate::lock::{is_lock_fresh, LOCK_FILENAME};
```

``` {.rust #project-lock-methods}
impl Project {
    /// Path to the lock file (`moonshot.lock`).
    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILENAME)
    }

    /// Returns `true` when the lock file exists and is newer than the
    /// manifest, meaning the solver does not need to run again.
    pub fn is_lock_fresh(&self) -> bool {
        is_lock_fresh(&self.lock_path(), &self.manifest_path)
    }
}
```

## The Session abstraction

Several commands (lock, install, and later build) need the same set of
resources: a `Project`, an HTTP client, a repodata `Gateway`, and a cache
directory. Rather than constructing these independently in every command, we
bundle them into a `Session` struct that is created once and threaded through
the pipeline.

`Session` also owns the resolve logic that was previously in a standalone
`resolve.rs` module. By making resolution a method on `Session`, every command
that needs a fresh solve can call `session.ensure_resolved(force)` and get
back either a cached lock or a freshly computed one.

### `src/session.rs`

``` {.rust file=src/session.rs}
<<session-imports>>
<<resolve-status-enum>>
<<session-struct>>
<<session-new>>
<<session-channels>>
<<session-resolve>>
<<session-ensure-resolved>>
```

The session needs the full resolution stack (HTTP, repodata, and the solver):

``` {.rust #session-imports}
use std::collections::HashMap;
use std::time::Instant;

use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::client::build_authenticated_client;
use crate::lock::{read_lock_file, read_locked_packages, write_lock_file};
use crate::progress::{with_spinner, with_spinner_sync};
use crate::project::Project;
```

The `ensure_resolved` method returns a `ResolveStatus` so that callers can
distinguish between a cached result and a fresh solve. The lock command uses
this to print different messages; the install command just extracts the
solution.

``` {.rust #resolve-status-enum}
/// The outcome of [`Session::ensure_resolved`].
pub enum ResolveStatus {
    /// The lock file was already up to date; no work was done.
    AlreadyFresh(Vec<RepoDataRecord>),
    /// A fresh solve was performed and the lock file was written.
    Resolved {
        solution: Vec<RepoDataRecord>,
        #[allow(dead_code)]
        platform: Platform,
    },
}
```

Two convenience methods extract the solution regardless of which variant we
have. `solution()` borrows it; `into_solution()` consumes the enum to avoid a
clone when ownership is available.

``` {.rust #resolve-status-enum}
#[allow(dead_code)]
impl ResolveStatus {
    /// Extract the solution regardless of which variant we are.
    pub fn solution(&self) -> &[RepoDataRecord] {
        match self {
            ResolveStatus::AlreadyFresh(s) => s,
            ResolveStatus::Resolved { solution, .. } => solution,
        }
    }

    pub fn into_solution(self) -> Vec<RepoDataRecord> {
        match self {
            ResolveStatus::AlreadyFresh(s) => s,
            ResolveStatus::Resolved { solution, .. } => solution,
        }
    }
}
```

The struct itself bundles a project with its networking resources:

``` {.rust #session-struct}
/// Bundles a [`Project`] with an HTTP client and repodata gateway.
#[allow(dead_code)]
pub struct Session {
    pub project: Project,
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub gateway: Gateway,
    pub cache_dir: std::path::PathBuf,
    pub channel_config: ChannelConfig,
}
```

Creating a session sets up the cache, HTTP client, and gateway:

``` {.rust #session-new}
impl Session {
    /// Create a new session from a discovered project.
    pub fn new(project: Project) -> miette::Result<Self> {
        let cache_dir = rattler::default_cache_dir()
            .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
        rattler_cache::ensure_cache_dir(&cache_dir)
            .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;

        let client = build_authenticated_client()?;
        let channel_config = ChannelConfig::default_with_root_dir(project.root.clone());
```

The `Gateway` is the central piece: it fetches, caches, and serves repodata.
We enable sharded repodata by default, which lets the gateway fetch only the
subset of packages that match our query instead of downloading the full index.

``` {.rust #session-new}
        let gateway = Gateway::builder()
            .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
            .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
            .with_client(client.clone())
            .with_channel_config(rattler_repodata_gateway::ChannelConfig {
                default: SourceConfig {
                    sharded_enabled: true,
                    ..SourceConfig::default()
                },
                per_channel: HashMap::new(),
            })
            .finish();

        Ok(Self {
            project,
            client,
            gateway,
            cache_dir,
            channel_config,
        })
    }
```

A helper parses the manifest's channel strings into typed values:

``` {.rust #session-channels}
    /// Parse the manifest channels into typed [`Channel`] values.
    pub fn channels(&self) -> miette::Result<Vec<Channel>> {
        self.project
            .manifest
            .project
            .channels
            .iter()
            .map(|s| Channel::from_str(s, &self.channel_config))
            .collect::<Result<_, _>>()
            .into_diagnostic()
            .context("parsing channels")
    }
```

`resolve` runs the full dependency-resolution pipeline: parse specs from the
manifest, fetch repodata recursively, detect virtual packages, and run the
solver. It returns the solution, channels, and platform so that callers can
write the lock file.

``` {.rust #session-resolve}
    /// Run the full dependency-resolution pipeline.
    pub async fn resolve(
        &self,
        locked_packages: Vec<RepoDataRecord>,
    ) -> miette::Result<(Vec<RepoDataRecord>, Vec<Channel>, Platform)> {
        let specs = self.project.manifest.match_specs()?;
        let channels = self.channels()?;
        let platform = Platform::current();

        let repo_data: Vec<RepoData> = with_spinner(
            "Fetching repodata",
            self.gateway
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

Before we can solve, we need to know what the current system provides.
Virtual packages like `__linux`, `__glibc`, and `__cuda` let the solver
exclude packages that need features the host doesn't have.

``` {.rust #session-resolve}
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

Now we assemble a `SolverTask` and hand it to resolvo. The `locked_packages`
field seeds the solver with previous solutions as preferences, which makes
re-solves faster and more stable.

``` {.rust #session-resolve}
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
    }
```

`ensure_resolved` is the main entry point for commands that need a solved
environment. It checks freshness, reads any existing lock records as solver
preferences, resolves if needed, and writes the lock file.

``` {.rust #session-ensure-resolved}
    /// Ensure the lock file is up to date, resolving if necessary.
    pub async fn ensure_resolved(&self, force: bool) -> miette::Result<ResolveStatus> {
        let lock_path = self.project.lock_path();
        let platform = Platform::current();

        if !force && self.project.is_lock_fresh() {
            let records = read_lock_file(&lock_path, platform)?;
            return Ok(ResolveStatus::AlreadyFresh(records));
        }

        let existing = read_locked_packages(&lock_path, platform);
        let (solution, channels, platform) = self.resolve(existing).await?;

        write_lock_file(&lock_path, &channels, platform, &solution)?;

        Ok(ResolveStatus::Resolved { solution, platform })
    }
}
```

### `src/commands/lock.rs`

With `Session` handling the heavy lifting, the lock command becomes a thin
wrapper: discover the project, create a session, and call `ensure_resolved`.

``` {.rust file=src/commands/lock.rs}
<<lock-cmd-imports>>
<<lock-cmd-args>>
<<lock-cmd-execute>>
```

The lock command's imports are minimal since it delegates to `Session`:

``` {.rust #lock-cmd-imports}
use clap::Parser;

use crate::lock::LOCK_FILENAME;
use crate::project::Project;
use crate::session::{ResolveStatus, Session};
```

A single `--force` flag controls whether to re-solve unconditionally:

``` {.rust #lock-cmd-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Force re-solving even if the lock is up to date.
    #[clap(long)]
    pub force: bool,
}
```

The execute function discovers the project, creates a session, and delegates:

``` {.rust #lock-cmd-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let session = Session::new(project)?;

    match session.ensure_resolved(args.force).await? {
        ResolveStatus::AlreadyFresh(_) => {
            println!("{} Lock is already up to date", console::style("✔").green());
        }
        ResolveStatus::Resolved { ref solution, .. } => {
            println!(
                "{} Wrote {} ({} packages)",
                console::style("✔").green(),
                LOCK_FILENAME,
                console::style(solution.len()).cyan()
            );
        }
    }

    Ok(())
}
```

## Testing

Let's add unit tests in `src/lock.rs` to verify that our write/read roundtrip
and the freshness check work correctly.

``` {.rust #lock-tests}
#[cfg(test)]
mod tests {
    use super::*;
    use fs_err as fs;
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
```

The first test writes a lock file with one package and reads it back, checking
that the name survives the roundtrip:

``` {.rust #lock-tests}
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
```

The second test exercises the timestamp-based freshness logic, stepping through
each state: no files, only manifest, lock newer, manifest newer.

``` {.rust #lock-tests}
    #[test]
    fn freshness_check() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("moonshot.toml");
        let lock_path = dir.path().join(LOCK_FILENAME);

        // Neither file exists → stale.
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Only manifest exists → stale.
        fs::write(&manifest_path, "").unwrap();
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Lock written after manifest → fresh.
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&lock_path, "").unwrap();
        assert!(is_lock_fresh(&lock_path, &manifest_path));

        // Manifest touched after lock → stale.
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&manifest_path, "changed").unwrap();
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

## When the solver says no

Not every set of dependencies has a solution. Getting solver errors that are both understandable and actionable is really important, but also pretty challenging. For a deep look at the complexity involved, see [this blog post on managing conflicts with mamba](https://medium.com/@AntoineProuvost/managing-conflicts-with-mamba-6a5fa10ed6a). Suppose you write this manifest:

```toml
[dependencies]
lua = ">=5.4,<5.5"
luajit = "*"
```

`lua` and `luajit` are different implementations of the Lua language. They
provide different packages and can't coexist. The solver detects that
their constraints are incompatible:

```console
$ shot lock
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
Error:
  × solving dependencies
  ╰─▶ The following packages are incompatible:
        - lua >=5.4,<5.5 can be satisfied by lua 5.4.7
        - luajit * can be satisfied by luajit 2.1
        - lua 5.4.7 conflicts with luajit 2.1 because they
          both provide the lua runtime
```

The error traces the chain of incompatibilities from your direct requests back
to the conflict. To read it: start from the bottom. The conflict tells you
*why* no solution exists. The lines above show which of your dependencies led
there.

Fixing it usually means relaxing one constraint or removing a dependency. In
this case, pick either `lua` or `luajit`, not both. For harder cases where
transitive dependencies conflict, the error message shows which intermediate
packages are involved so you know where to look.

[resolvo] generates these explanations by tracing its conflict graph, the same
CDCL machinery described in
[Deep Dive: The resolvo SAT Solver](deep-dive-resolvo.md). The explanation
isn't just "no solution"; it's the minimal set of constraints that cannot be
satisfied together.

## Exercises

!!! exercise-easy "Print Solve Solution Table"

    After resolving, print a formatted table showing every package in the solution. For each `RepoDataRecord`, display: package name, version, build string, and subdir.

    /// margin-note
    Each `RepoDataRecord` has a `package_record` with name, version, build, and subdir fields. Use `as_normalized()` to display package names. Add printing after `ensure_resolved` in `lock.rs`.
    ///

    Acceptance criteria
    :   - `shot lock` prints an aligned table (name, version, build, subdir)
        - Columns are aligned
        - Count matches the "Solved N packages" message

!!! exercise-intermediate "Lock File Diff"

    When re-locking (lock file already exists), compare the old and new solutions and print a diff. Read the old lock file before resolving, then compare package names and versions between old and new. Show added (+), removed (-), and upgraded/downgraded (~) packages.

    /// margin-note
    Read the old lock file before resolving. Build a HashMap of name-to-version for both old and new solutions, then diff the maps. Note that `PackageName` does not implement `Display`; use `.as_normalized()` for printing.
    ///

    Acceptance criteria
    :   - Adding a dependency then running `shot lock --force` shows `+ newpkg 1.0.0`
        - Removing a dependency shows `- oldpkg 2.0.0`
        - Version changes show `~ pkg 1.0.0 -> 1.1.0`
        - No changes shows "Lock file unchanged"

!!! exercise-hard "Virtual Package Overrides via Manifest"

    Add a `[virtual-packages]` table to `moonshot.toml` where users can override detected virtual packages for solving. This lets users target older systems (e.g., `__glibc = "2.17"` for manylinux2014 compatibility). Parse the table, construct `GenericVirtualPackage` values, and inject them into the `SolverTask` instead of auto-detected ones.

    /// margin-note
    Add a `virtual_packages` HashMap to `Manifest` (with the Serde rename from the recurring patterns note). Build `GenericVirtualPackage` values from the table entries. Start from `VirtualPackage::detect(...)` defaults, then replace matching names with the manifest overrides. Wire the result into `SolverTask` in `src/session.rs`.
    ///

    Acceptance criteria
    :   - Adding `[virtual-packages]` with `__glibc = "2.17"` to moonshot.toml makes the solver use glibc 2.17
        - Multiple overrides in the table work: `__glibc = "2.17"` and `__cuda = "11.8"`
        - Non-overridden virtual packages (e.g., `__unix`) are preserved from detection
        - Invalid package names (missing `__` prefix) or unparseable versions produce clear errors
        - `shot lock` reads the table and applies overrides before solving

## Summary

- `shot lock` resolves dependencies and writes `moonshot.lock`.
- If the lock is already fresh, the command exits immediately.
- `Session` bundles a `Project`, HTTP client, and repodata gateway. Its
  `ensure_resolved` method handles the full resolve pipeline and lock writing.
- `is_lock_fresh` compares modification times to decide whether to re-solve.
- `read_lock_file` extracts `RepoDataRecord`s from the lock via [rattler_lock].
- `write_lock_file` builds a `LockFile` from the solver output and writes YAML.

In the next chapter we implement `shot install`, which uses the lock file
as its source of truth for installation.

[resolvo]: https://github.com/mamba-org/resolvo
[rattler_lock]: https://crates.io/crates/rattler_lock
[pixi]: https://pixi.sh
[rattler_virtual_packages]: https://crates.io/crates/rattler_virtual_packages
