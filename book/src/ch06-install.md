# Chapter 6: Installing Packages

We have the solver's output: an exact list of packages to install.  Now we hand
it to rattler's `Installer` and let it do the heavy lifting.

## What installation means

Installing a conda package into a prefix is not just "unzip the archive into
`/usr/local`".  Several things make it more involved:

**1. The package cache**

Every package is first extracted into a *central cache* shared across all
environments on the machine (at `~/.rattler/pkgs/`).  The cache key is the
package's content hash, so `lua-5.4.7` is stored exactly once regardless of how
many environments use it. Content-addressed keys (rather than name-plus-version) prevent collisions when the same version is rebuilt with a different build string. Two builds of `lua-5.4.7` with different compiler flags get different hashes and coexist safely in the cache.

**2. Hard links**

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

**3. Transactions**

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

This makes `luapkg install` idempotent: running it twice with the same manifest
is a no-op.

## The `Installer` API

We configure the installer with a builder and then call `.install()` to apply the transaction.

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

### `with_reporter`

`IndicatifReporter` is a rattler-provided reporter that shows per-package
progress bars during download and extraction.  It integrates with `indicatif`'s
`MultiProgress` to correctly render multiple bars without interleaving.

You can implement your own reporter if you want custom progress display.  It's a
trait, not a concrete type.

### `with_execute_link_scripts`

Setting this to `true` tells the installer to run conda's **link scripts** after
installation.  These are scripts in `<prefix>/etc/conda/activate.d/` that some
packages use to set up post-install configuration (updating `LUA_PATH`, for
example).

### `with_requested_specs`

The installer needs to know which packages were *directly* requested (as opposed
to installed as transitive dependencies).  It records this in the
`conda-meta/*.json` files so that future updates can correctly distinguish "user
wants this" from "installed because something else needed it".

!!! info "Tracking direct vs transitive"

    This distinction drives automatic cleanup: when a direct dependency is
    removed, the installer can garbage-collect its transitive dependencies that
    nothing else needs. Both npm and pip added this tracking late in their
    development, and the lack of it caused years of accumulated orphan packages
    in user environments.

## Reading the result

Once installation finishes, we check the transaction to report whether anything changed.

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
        println!(
            "  Activate with:  eval $(luapkg shell)"
        );
    }

    Ok(())
```

`result.transaction.operations` is a list of what the installer did.
If it's empty, nothing changed.

All nine steps above live inside `install_from_manifest`:

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

The `execute` function is a thin entry point that reads the manifest and calls
`install_from_manifest`:

``` {.rust #install-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (_, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    install_from_manifest(&manifest, prefix).await
}
```

``` {.rust #install-private-helpers}
fn with_spinner_sync<T, F: FnOnce() -> T>(
    msg: &'static str,
    f: F,
) -> T {
    crate::progress::with_spinner_sync(msg, f)
}
```

## The `luapkg add` command

`add` is a thin wrapper around `install_from_manifest`:

``` {.rust file=src/commands/add.rs}
use std::env;

use clap::Parser;
use miette::IntoDiagnostic;

use crate::manifest::{Manifest, MANIFEST_FILENAME};

#[derive(Debug, Parser)]
pub struct Args {
    /// Packages to add, e.g. `luarocks` or `"lua >=5.4"`.
    #[clap(required = true)]
    pub packages: Vec<String>,

    /// Override the target prefix.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);
    let (_, mut manifest) = Manifest::find_in_dir(&cwd)?;

    let mut added = 0usize;
    for pkg in &args.packages {
        // Split "name version" or just "name".
        let (name, version) = split_spec(pkg);
        manifest
            .dependencies
            .entry(name.to_string())
            .or_insert_with(|| version.to_string());
        added += 1;
    }

    manifest.write(&manifest_path)?;
    println!(
        "{} Added {added} package(s) to `{MANIFEST_FILENAME}`",
        console::style("✔").green()
    );

    // Install immediately.
    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix).into_diagnostic()?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    super::install::install_from_manifest(&manifest, prefix).await
}

<<split-spec>>
```

### Parsing package specs

``` {.rust #split-spec}
fn split_spec(spec: &str) -> (&str, &str) {
    // Respect quoted strings and handle the common "name version" pattern.
    if let Some(pos) = spec.find(|c: char| c.is_whitespace() || c == '=') {
        let name = spec[..pos].trim();
        let version = spec[pos..].trim().trim_start_matches('=').trim();
        (name, if version.is_empty() { "*" } else { version })
    } else {
        (spec.trim(), "*")
    }
}
```

This splits `"lua >=5.4"` into `("lua", ">=5.4")` or `"luarocks"` into
`("luarocks", "*")`.

We split on the first whitespace or `=` character to separate the package name
from the version constraint.

## The prefix directory

``` {.rust file=src/commands/mod.rs}
pub mod add;
pub mod build;
pub mod init;
pub mod install;
pub mod run;
pub mod shell;

use std::path::{Path, PathBuf};

/// Return the path to the conda prefix managed by `luapkg`.
///
/// By convention we store the environment at `.luapkg/env/` relative to the
/// project root (the directory that contains `luapkg.toml`).  This is similar
/// to how pixi stores its environments in `.pixi/envs/`.
pub fn prefix_dir(project_root: &Path) -> PathBuf {
    project_root.join(".luapkg").join("env")
}
```

By convention, `luapkg` puts the environment at `.luapkg/env/` relative to the
project root, alongside `luapkg.toml`.  This keeps the environment close to the
project and out of the user's global namespace.

The user can override it with `--prefix /path/to/env`.

## What gets installed where

After `luapkg install`, the prefix looks like this:

```text
.luapkg/env/
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

- The `Installer` computes a transaction (diff) and applies only the changes.
- Files are hard-linked from the central cache into the prefix.
- The `Installer` builder lets you configure complex objects step by step.

In the next chapter we see how to *use* the installed environment by generating
a shell activation script.
