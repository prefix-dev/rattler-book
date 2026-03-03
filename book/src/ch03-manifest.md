# Chapter 3: The Manifest — Reading `luapkg.toml`

Every `luapkg` project is described by a single file: `luapkg.toml`.  In this
chapter we implement the `init` command (which creates it) and the `Manifest`
struct (which parses it).  Along the way we cover serde, error handling with
miette, and Rust's `Option` type.

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

## Rust concept: structs and serde

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

The `#[derive(Serialize, Deserialize)]` attributes tell serde how to convert
between the struct and any serialization format (TOML, JSON, YAML, …).  We use
TOML for the manifest file.

### `#[serde(default)]`

When a field is marked `#[serde(default)]`, serde uses the type's `Default`
implementation if the key is absent in the file.  For `HashMap`, the default is
an empty map — so `[dependencies]` is optional.

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

## Rust concept: error handling with `miette`

Rust has no exceptions.  Errors are values, returned as `Result<T, E>`.

For library code, the idiomatic error type is something from `thiserror` — an
enum with variants for each distinct error condition.  But for CLI *application*
code, we want to display errors nicely to the user without enumerating every
possible failure.  That's what `miette` is for.

`miette::Result<T>` is an alias for `Result<T, miette::Report>`.  A `Report`
can wrap *any* error that implements `std::error::Error`, and it knows how to
render it with colors, source location pointers, and contextual messages.

```rust
// Convert any std error → miette::Report
.into_diagnostic()?

// Attach extra context (shown in the error output)
.with_context(|| "while reading the manifest")?
```

If the user runs `luapkg install` without a `luapkg.toml`, they see:

```
Error: No `luapkg.toml` found in `/home/user/my-project`.
       Run `luapkg init` to create one.
```

Compare this to a bare `unwrap()` panic with a cryptic backtrace.  `miette` is
what makes a tool feel polished.

### The `bail!` macro

Sometimes you detect an error condition yourself rather than propagating a
library error:

```rust
if manifest_path.exists() {
    miette::bail!(
        "`{MANIFEST_FILENAME}` already exists in `{}`. \
         Delete it first if you want to re-initialise.",
        cwd.display()
    );
}
```

`bail!` constructs a `miette::Report` from a format string and returns it
immediately from the current function.  It's equivalent to:

```rust
return Err(miette::miette!("...").into());
```

## Rust concept: `Option<T>`

Looking at the `init` command:

```rust
#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the project.  Defaults to the current directory name.
    pub name: Option<String>,
    ...
}
```

`Option<String>` is how Rust expresses "this argument may or may not be
provided".  It's an enum with two variants: `Some(String)` and `None`.

We use `unwrap_or_else` to supply a fallback:

```rust
let name = args.name.unwrap_or_else(|| {
    cwd.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-lua-project")
        .to_string()
});
```

`unwrap_or_else` takes a **closure** — an anonymous function written with `||`
syntax.  The closure is only called if `args.name` is `None`.  If it's `Some`,
the inner `String` is returned directly.

The chained `.and_then(|n| n.to_str())` pattern is idiomatic Option-chaining.
`file_name()` returns `Option<&OsStr>`, and `.to_str()` converts it to
`Option<&str>` (which can fail if the path contains non-UTF-8 bytes).  If
either step returns `None`, the whole chain short-circuits to `None`, and we fall
back to `"my-lua-project"`.

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

A few things to note:

**`HashMap::from([...])`** is a convenient way to construct a `HashMap` from an
array of tuples.  Each tuple is `(key, value)`.

**`console::style("✔").green()`** uses the `console` crate to color terminal
output.  It gracefully degrades when stdout isn't a terminal (redirected to a
file, CI, etc.).

**`Ok(())`**: every `async fn` that returns `miette::Result<()>` must end with
`Ok(())` (or an early return via `?` or `bail!`).  The `()` unit type is Rust's
"void" — a value that carries no information.

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

## The `pub const` pattern

```rust
pub const MANIFEST_FILENAME: &str = "luapkg.toml";
```

Constants in Rust are inlined at compile time.  Making it a `const` in
`manifest.rs` and importing it everywhere means there's a single source of truth
for the filename.  The `&str` type is a string slice — a reference to a string
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
- `Option<T>` represents optional values; `.unwrap_or_else(closure)` supplies
  defaults.
- `bail!` creates and returns an error from a format string.

In the next chapter we'll implement `luapkg install`, starting with the first
step: fetching package metadata from a conda channel.
