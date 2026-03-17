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

Packages in the cache are immutable after extraction. No tool or environment modifies them in place. This invariant is what makes hard-linking safe: multiple environments can share the same inodes because nobody writes to them.

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

A naive package manager that unpacks files one by one can leave an environment half-installed if the process is interrupted. Partial installs are one of the most common failure modes in package management and often require manual cleanup.

The Installer computes a **transaction**, a diff between the currently-installed
state and the desired state, and applies only the changes:
- Install packages not currently present
- Remove packages no longer needed
- Update packages whose version changed

This makes `luapkg install` idempotent: running it twice with the same manifest
is a no-op.

## The `Installer` API

```rust
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

This is the **builder pattern**, a common Rust idiom for constructing objects
with many optional parameters.  Each `with_*` method returns `Self`, enabling
the fluent chain.

The builder collects configuration; `.install(&prefix, solution)` does the
actual work asynchronously.

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
wants this" from "installed because something else needed it". This distinction drives automatic cleanup: when a direct dependency is removed, the installer can garbage-collect its transitive dependencies that nothing else needs. Both npm and pip added this tracking late in their development, and the lack of it caused years of accumulated orphan packages in user environments.

## Reading the result

```rust
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
}
```

`result.transaction.operations` is a list of what the installer did.
If it's empty, nothing changed.

## The `luapkg add` command

`add` is a thin wrapper around `install_from_manifest`:

```rust
// src/commands/add.rs

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);
    let (_, mut manifest) = Manifest::find_in_dir(&cwd)?;

    let mut added = 0usize;
    for pkg in &args.packages {
        let (name, version) = split_spec(pkg);
        manifest
            .dependencies
            .entry(name.to_string())
            .or_insert_with(|| version.to_string());
        added += 1;
    }

    manifest.write(&manifest_path)?;
    // ...
    super::install::install_from_manifest(&manifest, prefix).await
}
```

### Parsing package specs

```rust
fn split_spec(spec: &str) -> (&str, &str) {
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

`spec.find(|c: char| c.is_whitespace() || c == '=')` uses a closure as a
pattern.  `String::find` accepts anything that implements `Pattern`: a char, a
`&str`, or a closure `Fn(char) -> bool`.

## The `install_from_manifest` helper

Both `add` and `install` call the same underlying function:

```rust
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<()> {
    // 1. Parse MatchSpecs
    // 2. Locate cache dir
    // 3. Build HTTP client
    // 4. Parse channels
    // 5. Query repodata via the Gateway
    // 6. Detect virtual packages
    // 7. Read currently-installed packages
    // 8. Solve
    // 9. Install
}
```

Extracting shared logic into a function is the right call when two commands need
the same steps.  In Rust, functions are zero-cost: there's no overhead compared
to inlining the code.

## The prefix directory

```rust
// src/commands/mod.rs

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

```
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
- The builder pattern lets you configure complex objects step by step.

In the next chapter we see how to *use* the installed environment by generating
a shell activation script.
