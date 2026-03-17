# Chapter 3: The Manifest — Reading `luapkg.toml`

Every `luapkg` project is described by a single file: `luapkg.toml`.  In this
chapter we implement the `init` command (which creates it) and the `Manifest`
struct (which parses it).  We also cover serde, error handling with miette, and
Rust's `Option` type.

A manifest records human intent: which packages the user wants and from which channels. It is distinct from a lock file, which records the exact versions the solver chose, including transitive dependencies. The manifest says "I want `lua >=5.4`"; a lock file says "install `lua 5.4.7 build h5eee18b_0` from conda-forge, plus these 12 transitive dependencies at these exact versions." We implement only the manifest in luapkg, but understanding the distinction matters for any package manager design.

## What the manifest looks like

```toml
[project]
name     = "my-lua-app"
channels = ["conda-forge"]

[dependencies]
lua      = ">=5.4"
luarocks = "*"
```

The `[project]` section contains metadata; `[dependencies]` maps package names
to version constraints.

## Modeling the manifest

We model the manifest as nested Rust structs:

```rust
// src/manifest.rs

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMetadata,

    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,

    #[serde(default = "default_channels")]
    pub channels: Vec<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}
```

Channels are listed in the manifest rather than in a global config file. This means each project pins its own package sources, so moving the project to another machine does not silently pick up a different channel list.

Defaulting to `conda-forge` is an opinionated choice. conda-forge is the largest community channel and covers most packages, so it is the right default for getting started. A production package manager might require an explicit channel list to avoid surprises, but for luapkg the convenience outweighs the risk.

The `#[derive(Serialize, Deserialize)]` attributes tell serde how to convert
between the struct and any serialization format (TOML, JSON, YAML, ...).  We use
TOML for the manifest file.

### `#[serde(default)]`

When a field is marked `#[serde(default)]`, serde uses the type's `Default`
implementation if the key is absent in the file.  For `HashMap`, the default is
an empty map, so `[dependencies]` is optional.

```toml
# This is a valid luapkg.toml with no dependencies:
[project]
name = "empty-project"
```

### `#[serde(default = "function")]`

For fields where the Rust default isn't what you want, you can name a function:

```rust
#[serde(default = "default_channels")]
pub channels: Vec<String>,

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}
```

If `channels` is missing from the TOML, serde calls `default_channels()` to get
`["conda-forge"]`.

## Reading and writing TOML

TOML works well for manifests because it is readable without documentation, preserves comments on round-trip (with the right library), and has strict typing that catches mistakes like quoting a number.

```rust
pub fn from_path(path: &Path) -> miette::Result<Self> {
    let content = std::fs::read_to_string(path)
        .into_diagnostic()
        .with_context(|| format!("reading manifest at `{}`", path.display()))?;

    toml::from_str(&content)
        .into_diagnostic()
        .with_context(|| format!("parsing manifest at `{}`", path.display()))
}
```

`std::fs::read_to_string` returns `Result<String, std::io::Error>`.  We convert
that to `miette::Result` with `into_diagnostic()`, then attach a context message
with `with_context`.

## Implementing `luapkg init`

```rust
// src/commands/init.rs

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);

    if manifest_path.exists() {
        miette::bail!(
            "`{MANIFEST_FILENAME}` already exists in `{}`. ...",
            cwd.display()
        );
    }

    let name = args.name.unwrap_or_else(|| { /* ... */ });

    let manifest = Manifest {
        project: ProjectMetadata {
            name: name.clone(),
            channels: args.channel,
        },
        dependencies: HashMap::from([
            ("lua".to_string(), ">=5.4".to_string()),
        ]),
    };

    manifest.write(&manifest_path)?;

    println!(
        "{} Created `{MANIFEST_FILENAME}` for project \"{name}\"",
        console::style("✔").green()
    );
    println!("  Add packages with:  luapkg add <package>");
    println!("  Install them with:  luapkg install");

    Ok(())
}
```

**`HashMap::from([...])`** is a convenient way to construct a `HashMap` from an
array of tuples.  Each tuple is `(key, value)`.

**`console::style("✔").green()`** uses the `console` crate to color terminal
output.  It degrades gracefully when stdout isn't a terminal (redirected to a
file, CI, etc.).

**`Ok(())`**: every `async fn` that returns `miette::Result<()>` must end with
`Ok(())` (or an early return via `?` or `bail!`).  The `()` unit type is Rust's
"void", a value that carries no information.

## `Manifest::find_in_dir`

Commands other than `init` need to *find* the manifest, not create it.  We add a
helper:

```rust
pub fn find_in_dir(dir: &Path) -> miette::Result<(PathBuf, Self)> {
    let path = dir.join(MANIFEST_FILENAME);
    if !path.exists() {
        miette::bail!(
            "No `{MANIFEST_FILENAME}` found in `{}`. \
             Run `luapkg init` to create one.",
            dir.display()
        );
    }
    let manifest = Self::from_path(&path)?;
    Ok((path, manifest))
}
```

It returns a tuple `(PathBuf, Manifest)` because callers sometimes need to
*write back* to the same path (e.g., `luapkg add` modifies the manifest before
installing).

`find_in_dir` only looks in the directory you pass it. An alternative design, used by Cargo and npm, walks up the directory tree until it finds a manifest. Walk-up is convenient when you run commands from a subdirectory, but it introduces ambiguity: which manifest did the tool find? For luapkg we chose current-directory-only because it is simpler to reason about and avoids accidentally operating on a parent project.

## The `pub const` pattern

```rust
pub const MANIFEST_FILENAME: &str = "luapkg.toml";
```

Constants in Rust are inlined at compile time.  Making it a `const` in
`manifest.rs` and importing it everywhere means there's a single source of truth
for the filename.  The `&str` type is a string slice, a reference to a string
stored in the binary's read-only data segment.

## Running `luapkg init`

At this point you can build and run the first command:

```
$ cargo build
$ ./target/debug/luapkg init hello-lua
✔ Created `luapkg.toml` for project "hello-lua"
  Add packages with:  luapkg add <package>
  Install them with:  luapkg install
$ cat luapkg.toml
[project]
name = "hello-lua"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
```

## Summary

- `Manifest` is a plain Rust struct derived from `Serialize`/`Deserialize`.
- `serde` handles TOML reading and writing.
- `miette` provides friendly error messages with context.

In the next chapter we'll implement `luapkg install`, starting with the first
step: fetching package metadata from a conda channel.
