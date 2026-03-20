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

The command parses the spec and updates `[dependencies]` in `moonshot.toml`.
It does not install anything; run `shot install` afterward to fetch and install
the new packages.

## Concepts

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
}

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

## Summary

- `add` modifies the manifest only; run `shot install` to apply changes.
- Manifest updates are idempotent: adding an existing package is a no-op.

In the next chapter we implement `shot lock`, which resolves the packages
listed in the manifest and records the exact solution. `shot install` follows
in Chapter 7.
