# Chapter 3: The `init` Command

Every `luapkg` project starts with `luapkg init`. This command creates
`luapkg.toml`, the manifest that describes which packages the user wants and
from which channels to fetch them.

## Design

`luapkg init` creates a new manifest in the current directory:

```bash
$ luapkg init hello-lua
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

The command accepts an optional project name (defaults to the current directory
name) and one or more `--channel` flags. If `luapkg.toml` already exists, it
refuses to overwrite.

## Configuration: `luapkg.toml`

```toml
[project]
name     = "my-lua-app"
channels = ["conda-forge"]

[dependencies]
lua      = ">=5.4"
luarocks = "*"
```

The `[project]` section contains metadata; `[dependencies]` maps package names
to version constraints.  Version specs follow the conda MatchSpec mini-language:

| Spec          | Meaning                           |
|---------------|-----------------------------------|
| `"*"`         | any version                       |
| `">=5.4"`     | 5.4 or newer                      |
| `"5.4.*"`     | any 5.4.x release                 |
| `">=5.4,<6"`  | 5.4 series, exclusive upper bound |

A manifest records human intent: which packages the user wants and from which channels. It is distinct from a lock file, which records the exact versions the solver chose, including transitive dependencies. The manifest says "I want `lua >=5.4`"; a lock file says "install `lua 5.4.7 build h5eee18b_0` from conda-forge, plus these 12 transitive dependencies at these exact versions." We implement only the manifest in luapkg, but understanding the distinction matters for any package manager design.

## Concepts

### serde for configuration files

We model the manifest as nested Rust structs that derive `Serialize` and
`Deserialize`. [serde] handles the conversion between [TOML] text and Rust values
in both directions.

TOML works well for manifests because it is readable without documentation, preserves comments on round-trip (with the right library), and has strict typing that catches mistakes like quoting a number.

The `#[serde(default)]` annotations make `[dependencies]` optional (defaults to
an empty map), and `#[serde(default = "default_channels")]` on `channels` falls
back to `["conda-forge"]` when omitted.

### miette for error handling

`std::fs::read_to_string` returns `Result<String, std::io::Error>`.  We convert
that to `miette::Result` with `into_diagnostic()`, then attach a context message
with `with_context`. miette renders these as user-friendly error messages with
source context.

## Implementation

### `src/manifest.rs`

Here is the full `src/manifest.rs` assembled from the pieces we'll walk through:

``` {.rust file=src/manifest.rs}
<<manifest-imports>>

<<manifest-filename-const>>

<<manifest-structs>>

<<manifest-impl>>
```

#### Imports

``` {.rust #manifest-imports}
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
```

#### The manifest filename

``` {.rust #manifest-filename-const}
/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "luapkg.toml";
```

A single constant for the filename keeps it consistent across all commands.

#### Structs

``` {.rust #manifest-structs}
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

We model the manifest as nested Rust structs. `Manifest` maps directly to the
top-level TOML, and `ProjectMetadata` maps to the `[project]` section. The
`name` field is used only for display; it does not affect resolution or
installation.

The dependency values are kept as plain `String`s rather than parsed
immediately. `rattler_conda_types` parses them into `MatchSpec` values at
solve-time, so parse errors surface with full context instead of at
manifest-read time.

Channels are listed in the manifest rather than in a global config file. This means each project pins its own package sources, so moving the project to another machine does not silently pick up a different channel list.

!!! note "Why default to conda-forge?"

    conda-forge is the largest community channel and covers most packages, so
    it is the right default for getting started. A real package manager
    might require an explicit channel list to avoid surprises, but for luapkg
    the convenience outweighs the risk.

#### Methods

The three methods live in a single `impl` block:

``` {.rust #manifest-impl}
impl Manifest {
<<manifest-from-path>>

<<manifest-write>>

<<manifest-find-in-dir>>
}
```

Reading TOML:

``` {.rust #manifest-from-path}
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .with_context(|| format!("reading manifest at `{}`", path.display()))?;

        toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))
    }
```

Writing TOML:

``` {.rust #manifest-write}
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .context("serializing manifest")?;

        std::fs::write(path, content)
            .into_diagnostic()
            .with_context(|| format!("writing manifest to `{}`", path.display()))
    }
```

Commands other than `init` need to *find* the manifest, not create it:

``` {.rust #manifest-find-in-dir}
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

!!! info "Design choice: no walk-up"

    `find_in_dir` only looks in the directory you pass it. An alternative
    design, used by Cargo and npm, walks up the directory tree until it finds a
    manifest. Walk-up is convenient when you run commands from a subdirectory,
    but it introduces ambiguity: which manifest did the tool find? For luapkg we
    chose current-directory-only because it is simpler to reason about and
    avoids accidentally operating on a parent project.

### `src/commands/init.rs`

``` {.rust file=src/commands/init.rs}
use std::collections::HashMap;

use clap::Parser;
use miette::IntoDiagnostic;

use crate::manifest::{Manifest, ProjectMetadata, MANIFEST_FILENAME};

#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the project.  Defaults to the current directory name.
    pub name: Option<String>,

    /// Conda channels to search (can be repeated).
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);

    if manifest_path.exists() {
        miette::bail!(
            "`{MANIFEST_FILENAME}` already exists in `{}`. \
             Delete it first if you want to re-initialise.",
            cwd.display()
        );
    }

    // Use the supplied name or fall back to the directory name.
    let name = args.name.unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-lua-project")
            .to_string()
    });

    // Build a starter manifest with Lua pre-filled so the user has something
    // to work with immediately.
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

**`console::style("✔").green()`** uses the `console` crate to color terminal
output.  It degrades gracefully when stdout isn't a terminal (redirected to a
file, CI, etc.).

## Running `luapkg init`

At this point you can build and run the first command:

```bash
pixi run luapkg init hello-lua
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

[serde]: https://serde.rs
[TOML]: https://toml.io

In the next chapter we'll implement `luapkg search`, which queries a channel for
available packages.
