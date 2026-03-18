# Chapter 6: The `add` Command

`add` is the most common way to grow your dependency list. It modifies the
manifest and then installs in one step.

## Design

```console
$ shot add luarocks
✔ Added 1 package(s) to `moonshot.toml`
⠋ Fetching repodata
  ...
✔ Environment updated in 1.8s
```

You can also pass a version constraint inline:

```console
$ shot add "lua >=5.4"
```

The command parses the spec, updates `[dependencies]` in `moonshot.toml`, and
calls `install_from_manifest` (the same function `shot install` uses).

## Concepts

### Composing commands

`add` reuses `install_from_manifest` from [Chapter 5](ch05-install.md). After updating the
manifest it calls the same install pipeline, so all the repodata fetching,
solving, and installation logic stays in one place. This is a common pattern:
small commands compose by calling shared functions rather than duplicating code.

### Idempotent manifest updates

If the package is already in `[dependencies]`, `add` skips it (uses
`entry().or_insert_with()`). Running `shot add lua` twice does not create a
duplicate entry or change the existing version constraint.

## Implementation

### `src/commands/add.rs`

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

## Running `shot add`

```console
$ shot add luarocks
✔ Added 1 package(s) to `moonshot.toml`
⠋ Fetching repodata
  1842 repodata records loaded
⠋ Solving
  Solved 6 packages in 0.4s
✔ Environment updated in 1.8s
  Activate with:  eval $(shot shell)

$ cat moonshot.toml
[project]
name = "hello-lua"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
luarocks = "*"
```

## Summary

- `add` modifies the manifest and then runs the full install pipeline.
- `install_from_manifest` is shared between `install` and `add`.
- Manifest updates are idempotent: adding an existing package is a no-op.

In the next chapter we implement `shot shell`, which generates a shell
activation script so you can use the installed packages.
