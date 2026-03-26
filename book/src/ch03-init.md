# Chapter 3: The `init` Command

<span class="newthought">Let's create our first command</span> `shot init`. This command will create a
`moonshot.toml`, the manifest that describes which packages you want and
from which channels to fetch them.

## Design

`shot init` creates a new manifest in the current directory:

```console
$ shot init lumen-app
✔ Created `moonshot.toml` for project "lumen-app"
  Add packages with:  shot add <package>
  Install them with:  shot install
$ cat moonshot.toml
[project]
name = "lumen-app"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
```

We'll use this project throughout the book. In [Chapter 10](ch10-build.md) we'll build a library
that can process an image and install it here.

The command will accept an optional project name (defaults to the current directory
name) and one or more `--channel` flags. Pass `--library` to scaffold a
buildable package (adds a `[build]` section and `version`). If `moonshot.toml`
already exists, it will refuse to overwrite.

## Configuration: `moonshot.toml`

```toml
[project]
name     = "my-lua-app"
channels = ["conda-forge"]

[dependencies]
lua      = ">=5.4"
luarocks = "*"
```

The `[project]` section will contain the metadata: `[dependencies]` maps package names
to version constraints.  Version specs follow the conda MatchSpec mini-language:

| Spec          | Meaning                           |
|---------------|-----------------------------------|
| `"*"`         | any version                       |
| `">=5.4"`     | 5.4 or newer                      |
| `"5.4.*"`     | any 5.4.x release                 |
| `">=5.4,<6"`  | 5.4 series, exclusive upper bound |

The manifest will record what your intent is when requesting packages: which packages you want and from which channels. In this way it is distinct from a lock file, which records the exact versions the solver chose, including transitive dependencies. 

The manifest says:

> I want `lua >=5.4`

A lock file says:

> Install `lua 5.4.7 build h5eee18b_0` from conda-forge, plus these 12 transitive dependencies at these exact versions.

We will implement both in moonshot: the manifest here and the lock file in [Chapter 6](ch06-lock.md).


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
use rattler_conda_types::{MatchSpec, ParseMatchSpecOptions};
use serde::{Deserialize, Serialize};
```

#### The manifest filename

``` {.rust #manifest-filename-const}
/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "moonshot.toml";
```

A single constant for the filename keeps it consistent across all commands.

#### Structs

``` {.rust #manifest-structs}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMetadata,

    #[serde(default)]
    pub dependencies: HashMap<String, String>,

    /// Present only for buildable packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,
}
```

### Serde for configuration files

We model the manifest as nested Rust structs that derive `Serialize` and
`Deserialize`. [Serde] handles the conversion between [TOML] text and Rust values
in both directions.

1. Using `#[serde(default)]` annotations make `[dependencies]` and others that use it optional.
2. `#[serde(default = "default_channels")]` on `channels` falls
back to `["conda-forge"]` when omitted.
<details class="margin-note" markdown>
<summary>Why default to conda-forge?</summary>

[conda-forge](https://conda-forge.org/) is the largest community conda channel and contains a lot of packages, so
it is a good default for getting started. </details>

```{.rust #manifest-structs}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub name: String,

    #[serde(default = "default_channels")]
    pub channels: Vec<String>,

    /// Package version (required when [build] is present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// SPDX license identifier, e.g. "MIT".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// One-line package description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_channels() -> Vec<String> {
    vec!["conda-forge".to_string()]
}
```



`Manifest` maps directly to the
top-level TOML, and `ProjectMetadata` maps to the `[project]` section. The
`name` field is used only for display; it does not affect resolution or
installation.

The `version`, `license`, and `description` fields are all optional. The
`build` field references `BuildConfig`, which we define in
[Chapter 10](ch10-build.md) when we implement `shot build`. A consume-only
project can leave all of these out.

We keep the dependency values as plain `String`s for serde compatibility with
TOML, but validate them eagerly: `from_path` calls `match_specs()` after
deserialising so that a typo like `lua = ">==5.4"` is caught at
manifest-read time rather than later during resolution.

We list channels in the manifest rather than in a global config file. This means each project pins its own package sources, so moving the project to another machine doesn't silently pick up a different channel list.


#### Methods

The methods live in a single `impl` block:

``` {.rust #manifest-impl}
impl Manifest {
<<manifest-from-path>>
<<manifest-write>>
<<manifest-find-in-dir>>
<<manifest-build-helpers>>
<<manifest-spec-helpers>>
}
```

Reading TOML:

``` {.rust #manifest-from-path}
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        let content = std::fs::read_to_string(path)
            .into_diagnostic()
            .with_context(|| format!("reading manifest at `{}`", path.display()))?;

        let manifest: Self = toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))?;

        // Validate dependency specs eagerly so typos are caught on load.
        manifest.match_specs()?;

        Ok(manifest)
    }
```
### Miette for error handling

`std::fs::read_to_string` returns `Result<String, std::io::Error>`.  We convert
that to `miette::Result` with `into_diagnostic()`, then attach a context message
with `with_context`. [Miette] renders these as user-friendly error messages with
source context.

### Writing

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
                 Run `shot init` to create one.",
                dir.display()
            );
        }
        let manifest = Self::from_path(&path)?;
        Ok((path, manifest))
    }
```

It returns a tuple `(PathBuf, Manifest)` because callers sometimes need to
*write back* to the same path (e.g., `shot add` modifies the manifest before
installing).

<details class="margin-note" markdown>
<summary>Design choice: no walk-up</summary>

`find_in_dir` only looks in the directory you pass it. An alternative
design, used by Cargo and npm, walks up the directory tree until it finds a
manifest. Walk-up is convenient when you run commands from a subdirectory,
but it introduces ambiguity: which manifest did the tool find? </details>

### Parsing dependencies as match specs

Both the resolver and the installer need to turn the `name = "version"` pairs
from `[dependencies]` into typed `MatchSpec` values. Rather than duplicating
that logic in every command, we add two helpers directly on `Manifest`.

`match_specs` parses each dependency into a `MatchSpec` using rattler's
parser, and `dependency_strings` formats them as `"name version"` strings
(the format conda's `index.json` uses for the `depends` field).

``` {.rust #manifest-spec-helpers}
    /// Parse the `[dependencies]` table into a list of [`MatchSpec`]s.
    ///
    /// This is used by both the resolver and the installer to turn the
    /// human-friendly `name = "version"` pairs into typed specs.
    pub fn match_specs(&self) -> miette::Result<Vec<MatchSpec>> {
        let opts = ParseMatchSpecOptions::default();
        self.dependencies
            .iter()
            .map(|(name, version)| {
                let spec_str = if version == "*" {
                    name.clone()
                } else {
                    format!("{name} {version}")
                };
                MatchSpec::from_str(&spec_str, opts)
                    .into_diagnostic()
                    .with_context(|| format!("parsing spec `{spec_str}`"))
            })
            .collect()
    }

    /// Format dependencies as `"name version"` strings (or just `"name"`
    /// when the version is `"*"`).
    ///
    /// This is the format expected by conda's `index.json` `depends` field.
    pub fn dependency_strings(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .map(|(name, spec)| {
                if spec == "*" {
                    name.clone()
                } else {
                    format!("{name} {spec}")
                }
            })
            .collect()
    }
```

### `src/commands/init.rs`

``` {.rust file=src/commands/init.rs}
use std::collections::HashMap;

use clap::Parser;
use miette::IntoDiagnostic;

use crate::manifest::{BuildConfig, Manifest, ProjectMetadata, MANIFEST_FILENAME};

#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the project.  Defaults to the current directory name.
    pub name: Option<String>,

    /// Conda channels to search (can be repeated).
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,

    /// Scaffold a buildable library (adds [build] section and version).
    #[clap(long)]
    pub library: bool,
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
            version: if args.library {
                Some("0.1.0".to_string())
            } else {
                None
            },
            license: None,
            description: None,
        },
        dependencies: HashMap::from([("lua".to_string(), ">=5.4".to_string())]),
        build: if args.library {
            Some(BuildConfig::default())
        } else {
            None
        },
    };

    manifest.write(&manifest_path)?;

    println!(
        "{} Created `{MANIFEST_FILENAME}` for project \"{name}\"",
        console::style("✔").green()
    );
    if args.library {
        println!("  Build a package with:  shot build");
    }
    println!("  Add packages with:  shot add <package>");
    println!("  Install them with:  shot install");

    Ok(())
}
```

**`console::style("✔").green()`** uses the [console] crate to color terminal
output.  It degrades gracefully when stdout isn't a terminal (redirected to a
file, CI, etc.).

## Running `shot init`

At this point you can build and run the first command:

```console
$ pixi run shot init lumen-app
✔ Created `moonshot.toml` for project "lumen-app"
  Add packages with:  shot add <package>
  Install them with:  shot install
$ cat moonshot.toml
[project]
name = "lumen-app"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
```

## Summary

- `Manifest` is a plain Rust struct derived from `Serialize`/`Deserialize`.
- `Serde` handles TOML reading and writing.
- `Miette` provides friendly error messages with context.

[Serde]: https://serde.rs
[TOML]: https://toml.io
[Miette]: https://docs.rs/miette
[console]: https://crates.io/crates/console
[rattler_conda_types]: https://crates.io/crates/rattler_conda_types

## Exercises

!!! exercise-easy "Add a `requires-lua` Field"

    Add a top-level `requires-lua` field to `moonshot.toml` (similar to `requires-python` in pyproject.toml). This field is more ergonomic than putting the Lua constraint in `[dependencies]` because it expresses the Lua version as a project-level requirement, not a regular dependency. Parse and validate it through `MatchSpec::from_str`. The `shot init` command gets a `--lua-version` flag to set it.

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    The value is a version constraint, not a full match spec. Prepend `"lua "` to build a valid spec for `MatchSpec::from_str`. Use a Serde rename so the TOML key keeps its hyphen. See `Manifest::match_specs()` for the parsing pattern.
    </details>

    Acceptance criteria
    :   - `shot init --lua-version ">=5.1,<5.5"` writes `requires-lua = ">=5.1,<5.5"` to `[project]`
        - `shot init --lua-version "!!!invalid"` fails with a parse error before creating any file
        - Default (no flag) writes `requires-lua = ">=5.4"`
        - `Manifest` struct has a `requires_lua: Option<String>` field that round-trips through TOML

!!! exercise-intermediate "Detect and Record Virtual Packages"

    At init time, detect the system's virtual packages using `VirtualPackage::detect()` and print them to stdout. Write a `[system]` section into the manifest with the detected values (e.g., `glibc = "2.31"` on Linux, `osx = "15.0"` on macOS). This gives users visibility into what their build host provides, which matters when the project later resolves dependencies (Ch6) or builds platform-specific packages (Ch10).

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    Call `VirtualPackage::detect` and convert results to `GenericVirtualPackage`. Note that `__archspec` puts the architecture in `build_string`, not `version`. See `src/session.rs` for the detection pattern. Store results as a `HashMap` in the manifest.
    </details>

    Acceptance criteria
    :   - `shot init` prints detected virtual packages (e.g., `Detected: __glibc=2.31, __archspec=1=x86_64`)
        - Manifest contains `[system]` with key-value pairs for detected packages
        - On macOS `__osx` is recorded; on Linux `__glibc` is recorded
        - `[system]` is omitted from serialization when empty

!!! exercise-hard "Init with Gateway Validation"

    Add a `--validate` flag to `shot init` that queries the configured channels to verify the Lua version constraint is satisfiable before writing the manifest. This requires constructing an HTTP client, creating a `Gateway`, and querying for a `MatchSpec` matching the `requires-lua` value. If no matching Lua packages exist in the channel, abort with a clear error.

    Dependencies: Exercise 3.1 (uses the `requires-lua` field).

    <details class="margin-note" markdown>
    <summary>Hint</summary>

    Build an HTTP client (see `src/client.rs`), create a `Gateway`, and query for the lua spec. Follow the gateway pattern in `src/commands/search.rs`.
    </details>

    Acceptance criteria
    :   - `shot init --validate` succeeds when `lua >=5.4` exists on conda-forge
        - `shot init --validate --lua-version ">=99.0"` fails with "No Lua packages matching >=99.0 found in channels"
        - Without `--validate`, init works offline as before
        - The channels from `--channel` flags (or the default) are used for the query

**Recurring patterns in exercises.** Two patterns come up in many exercises throughout this book. (1) TOML conventions use hyphens (`requires-lua`), but Rust fields use underscores (`requires_lua`). Add `#[serde(rename = "requires-lua")]` to bridge the two whenever an exercise adds a hyphenated key to `moonshot.toml`. (2) When you add a field to `Manifest` or `ProjectMetadata`, the compiler will point you to every place that constructs the struct. The most common one is `src/commands/init.rs`. Later exercises will not always remind you of this; follow the compiler errors.

In the next chapter we'll implement `shot search`, which queries a channel for
available packages.
