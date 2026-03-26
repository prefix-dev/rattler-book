# Chapter 5: The `add` Command

`add` is the most common way to grow your dependency list. It modifies the
manifest so you can then run `shot install` to apply the changes.

## Design

```console
$ shot add luarocks
✔ Added 1 package(s) to `moonshot.toml`
  Run `shot install` to apply changes.
```

You can also pass a version constraint inline:

```console
$ shot add "lua >=5.4"
```

The command parses the spec into a `NamelessMatchSpec`, and
updates `[dependencies]` in `moonshot.toml`. If any spec is malformed the
command aborts without modifying the manifest. It does not install anything;
run `shot install` afterward to fetch and install the new packages.

## Concepts

### Idempotent manifest updates

If the package is already in `[dependencies]`, `add` skips it (uses
`entry().or_insert_with()`). Running `shot add lua` twice does not create a
duplicate entry or change the existing version constraint.

## Implementation

The `add` command is where we first use the `Project` struct — a small
abstraction that finds the manifest from the current directory and provides
helpers for saving changes back to disk. We introduce `Project` in its own
file.

### `src/project.rs`

The `Project` struct centralises the boilerplate that every command repeats:
find the manifest, compute the project root, and derive paths from it. We
define it here and extend it with more methods in later chapters.

``` {.rust file=src/project.rs}
<<project-imports>>
<<project-struct>>
<<project-impl>>
```

``` {.rust #project-imports}
use crate::manifest::Manifest;
use miette::IntoDiagnostic;
use std::path::PathBuf;
```

``` {.rust #project-struct}
/// A discovered project on disk.
///
/// Bundles the project root, manifest path, and parsed manifest into a
/// single value so that every command does not have to repeat the same
/// discovery boilerplate.
pub struct Project {
    /// The directory that contains `moonshot.toml`.
    pub root: PathBuf,
    /// Absolute path to `moonshot.toml`.
    pub manifest_path: PathBuf,
    /// The parsed manifest.
    pub manifest: Manifest,
}
```

``` {.rust #project-impl}
impl Project {
    /// Discover the project from the current working directory.
    pub fn discover() -> miette::Result<Self> {
        let cwd = std::env::current_dir().into_diagnostic()?;
        let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;
        let root = manifest_path
            .parent()
            .expect("manifest path always has a parent")
            .to_path_buf();
        Ok(Self {
            root,
            manifest_path,
            manifest,
        })
    }

    /// The default prefix directory for the conda environment.
    pub fn default_prefix(&self) -> PathBuf {
        self.root.join(".env")
    }

    /// Persist the (possibly modified) manifest back to disk.
    pub fn save(&self) -> miette::Result<()> {
        self.manifest.write(&self.manifest_path)
    }
}
```

### `src/commands/add.rs`

``` {.rust file=src/commands/add.rs}
use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::NamelessMatchSpec;

use crate::manifest::MANIFEST_FILENAME;
use crate::project::Project;

#[derive(Debug, Parser)]
pub struct Args {
    /// Packages to add, e.g. `luarocks` or `"lua >=5.4"`.
    #[clap(required = true)]
    pub packages: Vec<String>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let mut project = Project::discover()?;

    // Validate all specs before modifying the manifest.
    let parsed: Vec<(&str, NamelessMatchSpec)> = args
        .packages
        .iter()
        .map(|pkg| {
            let (name, version) = split_spec(pkg);
            let spec: NamelessMatchSpec = version
                .parse()
                .into_diagnostic()
                .with_context(|| format!("invalid dependency spec `{pkg}`"))?;
            Ok((name, spec))
        })
        .collect::<miette::Result<_>>()?;

    let mut added = 0usize;
    for (name, spec) in parsed {
        project
            .manifest
            .dependencies
            .entry(name.to_string())
            .or_insert(spec);
        added += 1;
    }

    project.save()?;
    println!(
        "{} Added {added} package(s) to `{MANIFEST_FILENAME}`",
        console::style("✔").green()
    );
    println!("  Run `shot install` to apply changes.");
    Ok(())
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

## Running `shot add`

```console
$ shot add luarocks
✔ Added 1 package(s) to `moonshot.toml`
  Run `shot install` to apply changes.

$ cat moonshot.toml
[project]
name = "hello-lua"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
luarocks = "*"
```

## Exercises

!!! exercise-easy "Warn on Version Constraint Change"

    Currently `shot add "lua >=5.3"` silently keeps the existing `>=5.4` constraint because of `or_insert_with`. Change `add` so it warns the user when a package already exists with a *different* version constraint, and add a `--force` flag that overwrites the existing constraint.

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    Use `entry().and_modify()` or check the existing value before inserting. Add `#[clap(long)]` for the `--force` flag. Print the old and new constraints in the warning.
    </details>

    Acceptance criteria
    :   - `shot add lua` when `lua = ">=5.4"` already exists prints a warning and keeps `>=5.4`
        - `shot add "lua >=5.3"` prints "lua already has constraint `>=5.4`, skipping (use --force to overwrite)"
        - `shot add --force "lua >=5.3"` replaces the constraint with `>=5.3`
        - Adding a truly new package still works as before

!!! exercise-intermediate "Validate Package Exists in Channel Before Adding"

    Make `shot add` query the repodata gateway by default to verify each package exists in the configured channels before adding it. If a package is not found, refuse to add it. Construct a `Session`, query with the parsed `MatchSpec`, and check that at least one matching record comes back. Add `--offline` to skip the check for users without network access.

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    Create a `Session` to get gateway access and query for the package. Check whether the returned repodata has any records. Follow the pattern in `src/commands/search.rs`. Note that `Session::new` consumes the `Project`, so you may need to re-discover it afterward.
    </details>

    Acceptance criteria
    :   - `shot add lua` queries conda-forge and succeeds (lua exists)
        - `shot add nonexistent-package-xyz` fails with "Package not found in channels: ..."
        - `shot add --offline lua` skips the gateway check and adds without validation
        - The manifest's configured channels are used for the query

!!! exercise-hard "Platform-Specific Dependencies"

    Implement `shot add --platform linux-64 lua` which adds the dependency to a platform-specific table `[platform-dependencies.linux-64]` instead of the global `[dependencies]`. This requires extending the `Manifest` struct with a `platform_dependencies: HashMap<String, HashMap<String, String>>` field, parsing the target platform with `Platform::from_str`, and optionally validating via the gateway for that specific platform.

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    Use `Platform::from_str` to validate platform strings. Add a `platform_dependencies` HashMap to `Manifest` (with the Serde rename from the recurring patterns note). Route the `--platform` flag in `src/commands/add.rs` to write to the platform-specific table instead of `[dependencies]`.
    </details>

    Acceptance criteria
    :   - `shot add --platform linux-64 lua` writes to `[platform-dependencies.linux-64]`
        - `shot add --platform linux-64` validates the package exists for linux-64 specifically (gateway is on by default)
        - Without `--platform`, behavior is unchanged (adds to `[dependencies]`)
        - Invalid platform strings produce a clear error
        - Multiple `--platform` flags add to each platform section

## Summary

- `add` modifies the manifest only; run `shot install` to apply changes.
- Manifest updates are idempotent: adding an existing package is a no-op.

In the next chapter we implement `shot lock`, which resolves the packages
listed in the manifest and records the exact solution. `shot install` follows
in Chapter 7.
