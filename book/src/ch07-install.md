# Chapter 7: The `install` Command

Now we get to the heart of moonshot: the install command. It reads the manifest,
checks the lock file, resolves if needed, and installs packages into a local
prefix. The lock file (produced by [Chapter 6](ch06-lock.md)'s `shot lock`) is
our source of truth: if it's fresh, `shot install` replays it without touching
the network or the solver.

## Design

`shot install` runs the full pipeline: check lock, resolve if stale, install.

```console
$ shot install
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
✔ Wrote moonshot.lock (5 packages)
  Downloading and extracting packages...
✔ Environment updated in 2.1s
  Activate with:  eval $(shot shell)
```

When the lock is fresh, the resolve step is skipped entirely:

```console
$ shot install
  Downloading and extracting packages...
✔ Environment already up to date
```

You can pass an optional `--prefix` flag to override the install
location. By default, packages go into `.env/` relative to the
project root.

## Configuration

### The prefix directory

By convention, `moonshot` puts the environment at `.env/` relative to the
project root, alongside `moonshot.toml`.  This keeps the environment close to the
project and out of the user's global namespace. The user can override it with
`--prefix /path/to/env`. The `prefix_dir` helper and command module declarations
are defined in [Chapter 2](ch02-project-setup.md).

## Concepts: Installation

### The package cache

Every package is first extracted into a *central cache* shared across all
environments on your machine (at `~/.rattler/pkgs/`).  The cache key is the
package's content hash, so `lua-5.4.7` is stored exactly once regardless of how
many environments use it. Content-addressed keys (rather than name-plus-version) prevent collisions when the same version is rebuilt with a different build string. Two builds of `lua-5.4.7` with different compiler flags get different hashes and coexist safely in the cache.

### Hard links and reflinks

!!! info "Why hard-linking is safe"

    Packages in the cache are immutable after extraction. No tool or environment
    modifies them in place. This invariant is what makes hard-linking safe:
    multiple environments can share the same inodes because nobody writes to
    them.

From the cache, files are linked into the target prefix. On most systems,
rattler uses **reflinks** (copy-on-write clones) when the filesystem supports
them (APFS on macOS, Btrfs and XFS on Linux). A reflink shares the underlying
data blocks without sharing the inode, so writing to one copy doesn't affect
the other. On filesystems without reflink support, rattler falls back to
**hard links**, which are a second directory entry pointing to the same inode.
If hard links are also unavailable (some network filesystems, Windows
cross-volume), it copies the file.

This means:

- An environment takes almost no disk space for packages that are already cached.
- Creating a new environment is very fast (linking is cheap).

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

<<install-run-installer>>
```

#### Imports

``` {.rust #install-imports}
use std::env;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler_conda_types::{
    MatchSpec, ParseMatchSpecOptions, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;

use crate::lock::{is_lock_fresh, read_lock_file, write_lock_file, LOCK_FILENAME};
use crate::manifest::Manifest;
use crate::resolve::{read_locked_packages, resolve_from_manifest};
```

Most of the gateway and solver imports are gone; the resolve pipeline lives
in `src/resolve.rs` ([Chapter 6](ch06-lock.md)).

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

This function resolves dependencies and installs them in one step. We'll reuse
it in the build command ([Chapter 10](ch10-build.md)) to install build-time
dependencies. It returns the solved records so callers can inspect what was
installed.

``` {.rust #install-from-manifest}
/// Shared install logic: resolve from the manifest, then install into `prefix`.
///
/// Pulling this out into its own function means the build command can call
/// it to install build dependencies without duplicating any networking or
/// solving code.
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<Vec<RepoDataRecord>> {
    let (solution, _channels, platform) =
        resolve_from_manifest(manifest, vec![]).await?;
    let result = solution.clone();
    run_installer(manifest, &prefix, solution, platform).await?;
    Ok(result)
}
```

`resolve_from_manifest` runs the full pipeline (specs, HTTP, gateway, solver)
and returns the solution. We clone the solution before handing it to the
installer, which consumes it. The build command calls this function without
going through the lock file, since build environments are temporary.

#### The execute function

The `execute` function is our entry point for `shot install`. It checks the
lock file before deciding whether to resolve.

``` {.rust #install-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    let lock_path = cwd.join(LOCK_FILENAME);
    let platform = Platform::current();

    let solution = if is_lock_fresh(&lock_path, &manifest_path) {
        read_lock_file(&lock_path, platform)?
    } else {
        let existing = read_locked_packages(&lock_path, platform);
        let (solution, channels, platform) =
            resolve_from_manifest(&manifest, existing).await?;
        write_lock_file(&lock_path, &channels, platform, &solution)?;
        println!(
            "{} Wrote {} ({} packages)",
            console::style("✔").green(),
            LOCK_FILENAME,
            console::style(solution.len()).cyan()
        );
        solution
    };

    run_installer(&manifest, &prefix, solution, platform).await
}
```

If the lock is fresh, `read_lock_file` returns the exact solution that was
recorded previously, and the solver is never invoked. If the lock is stale or
missing, we read whatever existing records the lock contains (for the solver's
`locked_packages` preference), resolve, write the new lock, and print a
confirmation message.

#### The installer

`run_installer` takes our solved set of packages and links them into the prefix.
We build a lightweight HTTP client for package downloads, scan the prefix for
already-installed packages (to compute a minimal transaction), and run the
`Installer`.

``` {.rust #install-run-installer}
async fn run_installer(
    manifest: &Manifest,
    prefix: &std::path::Path,
    solution: Vec<RepoDataRecord>,
    platform: Platform,
) -> miette::Result<()> {
    <<parse-install-specs>>
    <<install-client>>
    <<run-install>>
}
```

We re-parse the manifest specs so the installer knows which packages you
directly requested (as opposed to transitive dependencies pulled in by the
solver).

``` {.rust #parse-install-specs}
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

The HTTP client follows the same pattern as [Chapter 4](ch04-search.md) and
[Chapter 6](ch06-lock.md): `reqwest` with authentication middleware.

``` {.rust #install-client}
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

Finally we scan the prefix for already-installed packages, build a minimal
transaction, and run the `Installer`. It shows per-package progress bars via
`IndicatifReporter`.

``` {.rust #run-install}
let installed_packages =
    PrefixRecord::collect_from_prefix::<PrefixRecord>(prefix).into_diagnostic()?;

let start_install = Instant::now();
let result = Installer::new()
    .with_download_client(client)
    .with_target_platform(platform)
    .with_installed_packages(installed_packages)
    .with_execute_link_scripts(true)
    .with_requested_specs(specs)
    .with_reporter(IndicatifReporter::builder().finish())
    .install(prefix, solution)
    .await
    .into_diagnostic()
    .context("installing packages")?;

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

`IndicatifReporter` is a rattler-provided reporter that shows per-package
progress bars during download and extraction. If you want custom progress
display, you can implement your own; it's a trait, not a concrete type.

Setting `with_execute_link_scripts(true)` tells the installer to run conda's
**link scripts** after installation. These are scripts in
`<prefix>/etc/conda/activate.d/` that some packages use to set up post-install
configuration (updating `LUA_PATH`, for example).

The installer needs to know which packages you *directly* requested (as opposed
to transitive dependencies) via `with_requested_specs`. It records this in the
`conda-meta/*.json` files so that future updates can correctly distinguish
"you asked for this" from "installed because something else needed it".

!!! info "Tracking direct vs transitive"

    This distinction drives automatic cleanup: when a direct dependency is
    removed, the installer can garbage-collect its transitive dependencies that
    nothing else needs. Both npm and pip added this tracking late in their
    development, and the lack of it caused years of accumulated orphan packages
    in user environments.

## Running `shot install`

```console
$ shot install
⠋ Fetching repodata
  1523 repodata records loaded
⠋ Solving
  Solved 5 packages in 0.3s
✔ Wrote moonshot.lock (5 packages)
  Downloading and extracting packages...
✔ Environment updated in 2.1s
  Activate with:  eval $(shot shell)
```

### What gets installed where

After `shot install`, the prefix looks like this:

<div class="file-tree">
<ul>
  <li class="dir"><span class="name">.env/</span>
    <ul>
      <li class="dir"><span class="name">bin/</span>
        <ul>
          <li class="file"><span class="name">lua</span> <span class="comment">the Lua interpreter</span></li>
          <li class="file"><span class="name">luarocks</span> <span class="comment">LuaRocks (if installed)</span></li>
        </ul>
      </li>
      <li class="dir"><span class="name">lib/</span>
        <ul>
          <li class="file"><span class="name">liblua.so.5.4</span></li>
          <li class="file"><span class="name">…</span></li>
        </ul>
      </li>
      <li class="dir"><span class="name">share/</span>
        <ul>
          <li class="dir"><span class="name">lua/5.4/</span>
            <ul>
              <li class="file"><span class="name">…</span> <span class="comment">pure-Lua libraries</span></li>
            </ul>
          </li>
        </ul>
      </li>
      <li class="dir"><span class="name">conda-meta/</span>
        <ul>
          <li class="file"><span class="name">lua-5.4.7-h5eee18b_0.json</span></li>
          <li class="file"><span class="name">…</span> <span class="comment">one file per installed package</span></li>
        </ul>
      </li>
    </ul>
  </li>
</ul>
</div>

The `conda-meta/` directory is rattler's installation database.  Each JSON
file records the package name, version, build, all installed files, and their
hashes. You can inspect these to see exactly what's in your environment.

## Summary

- The install command checks the lock file before doing any work.
- If the lock is fresh, packages are installed directly from it (no solver,
  no network).
- If the lock is stale or missing, `resolve_from_manifest` runs the full
  pipeline and the result is written to `moonshot.lock` before installation.
- The `Installer` computes a transaction (diff) and applies only the changes.
- Files are linked from the central cache into the prefix, using reflinks
  where available and falling back to hard links or copies.
- `install_from_manifest` provides a one-call interface for the build command.

In the next chapter we set up shell hooks, which generate activation
scripts so you can use the installed packages.
