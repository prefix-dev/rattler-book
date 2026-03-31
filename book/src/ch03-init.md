# Chapter 3: The `init` Command

<span class="newthought">Let's create our first command</span> `shot init`. This command will create a
`moonshot.toml`, the manifest that describes which packages you want and
from which channels to fetch them. It's convenient to start with an init command, so that we can use it for testing and easily running the project.

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
library project (adds a `[build]` section and `version`). If `moonshot.toml`
already exists, it will refuse to overwrite.

## Configuration: `moonshot.toml`

```toml
[project]
name     = "my-lua-app"
channels = ["conda-forge"]
platforms = ["linux-64", "osx-arm64"]

[dependencies]
lua      = ">=5.4"
luarocks = "*"
```

The `[project]` section will contain the metadata: `[dependencies]` maps package names
to version constraints.  Version Requirements follow the conda MatchSpec mini-language, which evolved in conjunction with Python's Version and Requirements syntax. It does offer a couple of crazy features like matching on regexes, md5 hashes (with a regex even, never seen that being used!), and globs:

Some simple most-commonly used cases:

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

We begin with the standard imports for file handling, serialization, and conda types:

``` {.rust #manifest-imports}
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use fs_err as fs;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{MatchSpec, NamelessMatchSpec, PackageName, Platform};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
```

A single constant keeps the filename consistent across all commands:

``` {.rust #manifest-filename-const}
/// The file name we look for in the current directory.
pub const MANIFEST_FILENAME: &str = "moonshot.toml";
```

The core data structures map directly to the TOML layout. The struct below uses
[serde_with], a companion crate to serde, for the `dependencies` field:

- `#[serde_as]` on the struct enables `serde_with`'s custom (de)serialization.
- `#[serde_as(as = "DisplayFromStr")]` on a field tells serde to call
  `FromStr::from_str()` when reading and `Display::fmt` when writing.
- Version strings like `">=5.4"` in the TOML are automatically parsed into
  typed `NamelessMatchSpec` values at load time.

We use `BTreeMap` instead of `HashMap` so that dependencies serialize in
alphabetical order, producing stable diffs when the manifest changes.

The `platforms` field lists which platforms to solve for. By default it includes only the current platform, but you can add others (like `linux-64` or `osx-arm64`) to produce a lock file that works across machines.

``` {.rust #manifest-structs}
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMetadata,

    #[serde(default = "default_platforms")]
    pub platforms: Vec<Platform>,

    #[serde_as(as = "BTreeMap<_, DisplayFromStr>")]
    #[serde(default)]
    pub dependencies: BTreeMap<String, NamelessMatchSpec>,

    /// Present only for library projects.
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
/// margin-note
[conda-forge](https://conda-forge.org/) is the largest community conda channel and contains a lot of packages, so
it is a good default for getting started.
///

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

pub(crate) fn default_platforms() -> Vec<Platform> {
    vec![Platform::current()]
}
```

`Manifest` maps directly to the
top-level TOML, and `ProjectMetadata` maps to the `[project]` section. The
`name` field is used only for display; it does not affect resolution or
installation.

The `version`, `license`, and `description` fields are all optional. The
`build` field references `BuildConfig`, which we define in
[Chapter 10](ch10-build.md) when we implement `shot build`. An application
project can leave all of these out.

### `serde_with` and `DisplayFromStr`

The dependency values in TOML are plain strings like `">=5.4"`, but we want them
as typed `NamelessMatchSpec` values in Rust. The [serde_with] crate bridges this
gap with `DisplayFromStr`:

- **On read**: calls `NamelessMatchSpec::from_str` for each value
- **On write**: calls `Display::fmt` to turn it back into a string
- **Keys** are left alone (they're already `String`s)

This follows the "parse, don't validate" principle: convert raw data into typed
values at the boundary, so the rest of your code can assume validity without
re-checking. The payoff is early error detection: a typo like `lua = ">==5.4"`
is caught during TOML deserialization itself. If the spec string is malformed,
`toml::from_str` returns an error before `Manifest` is ever constructed.

`rattler_conda_types` already uses `serde_with` internally, so adding it here
does not introduce a new transitive dependency.

We list channels in the manifest rather than in a global config file.
Historically in conda and pip, a lot of config lives in your global
configuration. One of the visions we have with pixi is that global config
impedes reproducibility, so we try to put as much into the manifest as
possible. With your own package manager you are free to decide of course.


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
        let content = fs::read_to_string(path)
            .into_diagnostic()
            .context("reading manifest")?;

        // `DisplayFromStr` validates every dependency spec during
        // deserialization, so a typo like `">==5.4"` fails here.
        let manifest: Self = toml::from_str(&content)
            .into_diagnostic()
            .with_context(|| format!("parsing manifest at `{}`", path.display()))?;

        Ok(manifest)
    }
```
### `fs_err` and Miette for error handling

We use [`fs_err`][fs_err] instead of `std::fs` throughout this project, aliased
as `fs` with `use fs_err as fs;`. It is a drop-in replacement that wraps every
error with the file path that caused it, so you never see a bare "No such file
or directory" without knowing *which* file. We found this very useful, while working
on pixi so that the unknown directory or file at least surfaces.

`fs::read_to_string` returns `Result<String, fs_err::Error>`, which implements
`std::error::Error`.  We convert that to `miette::Result` with
`into_diagnostic()`, then optionally attach extra context with `.context()`.
[Miette] renders these as user-friendly error messages in the terminal.

/// margin-note
`.into_diagnostic()` confused me at first, but because the `miette::Result` differs from the
regular `Results` this is a way to convert between the two.
///

[fs_err]: https://docs.rs/fs-err

### Writing

Writing TOML:

``` {.rust #manifest-write}
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        let content = toml::to_string_pretty(self)
            .into_diagnostic()
            .context("serializing manifest")?;

        fs::write(path, content)
            .into_diagnostic()
            .context("writing manifest")
    }
```

Commands other than `init` need to *find* the manifest, not create it, so lets have a method for it:

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

/// margin-note
`find_in_dir` only looks in the directory you pass it. An alternative
design, used by Cargo and npm, walks up the directory tree until it finds a
manifest. Walk-up is convenient when you run commands from a subdirectory,
but it introduces ambiguity: which manifest did the tool find?
///

### Parsing dependencies as match specs

Both the resolver and the installer need to turn the `name = "version"` pairs
from `[dependencies]` into typed `MatchSpec` values. Rather than duplicating
that logic in every command, we add two helpers directly on `Manifest`.

`match_specs` combines each key-value pair into a full `MatchSpec` using
`MatchSpec::from_nameless`. Because the values are already parsed
`NamelessMatchSpec`s (thanks to `DisplayFromStr`), no string concatenation
or re-parsing is needed. `dependency_strings` formats them as
`"name version"` strings (the format conda's `index.json` uses for the
`depends` field).

``` {.rust #manifest-spec-helpers}
    /// Combine each `[dependencies]` entry into a full [`MatchSpec`].
    ///
    /// The values are already parsed `NamelessMatchSpec`s, so this just
    /// attaches the package name.
    pub fn match_specs(&self) -> miette::Result<Vec<MatchSpec>> {
        self.dependencies
            .iter()
            .map(|(name, spec)| {
                let name = PackageName::from_str(name)
                    .into_diagnostic()
                    .with_context(|| format!("invalid package name `{name}`"))?;
                Ok(MatchSpec::from_nameless(spec.clone(), name.into()))
            })
            .collect()
    }
```

The second helper formats dependencies as `"name version"` strings for conda's
`index.json`. We use this in [Chapter 10](ch10-build.md) when writing package
metadata.

``` {.rust #manifest-spec-helpers}
    /// Format dependencies as `"name version"` strings (or just `"name"`
    /// when there is no version constraint).
    ///
    /// This is the format expected by conda's `index.json` `depends` field.
    pub fn dependency_strings(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .map(|(name, spec)| {
                if spec.version.is_none() {
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
<<init-imports>>
<<init-args>>
<<init-execute>>
```

The imports pull in clap for argument parsing and the manifest types we just
defined:

``` {.rust #init-imports}
use std::collections::BTreeMap;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::NamelessMatchSpec;

use crate::manifest::{
    default_platforms, BuildConfig, Manifest, ProjectMetadata, MANIFEST_FILENAME,
};
```

The `Args` struct uses clap's derive macros. The `--library` flag controls
whether a `[build]` section is scaffolded:

``` {.rust #init-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the project.  Defaults to the current directory name.
    pub name: Option<String>,

    /// Conda channels to search (can be repeated).
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,

    /// Scaffold a library project (adds [build] section and version).
    #[clap(long)]
    pub library: bool,
}
```

The execute function first checks that no manifest exists yet, then resolves
the project name (from the argument or the directory name):

``` {.rust #init-execute}
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
```

With the name resolved, we construct a starter `Manifest` with Lua pre-filled
so the user has something to work with immediately. When `--library` is set we
also add a version and a default `BuildConfig`:

``` {.rust #init-execute}
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
        platforms: default_platforms(),
        dependencies: BTreeMap::from([(
            "lua".to_string(),
            ">=5.4".parse::<NamelessMatchSpec>().unwrap(),
        )]),
        build: if args.library {
            Some(BuildConfig::default())
        } else {
            None
        },
    };
```

Finally we write the manifest and print a short guide for the user:

``` {.rust #init-execute}
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

We use the [console] crate to color terminal
output.  It degrades gracefully when stdout isn't a terminal (redirected to a
file, CI, etc.), again another useful rust library.

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
- `serde_with`'s `DisplayFromStr` bridges `FromStr`/`Display` types to serde, giving us typed dependency values for free.
- `Miette` provides friendly error messages with context.

[Serde]: https://serde.rs
[serde_with]: https://docs.rs/serde_with
[TOML]: https://toml.io
[Miette]: https://docs.rs/miette
[console]: https://crates.io/crates/console
[rattler_conda_types]: https://crates.io/crates/rattler_conda_types

## Exercises

Before we start, there is a small thing to take into account when starting:

**Recurring patterns in exercises.** Two patterns come up in many exercises throughout this book:

1. TOML conventions use hyphens (`requires-lua`), but Rust fields use underscores (`requires_lua`). Add `#[serde(rename = "requires-lua")]` to bridge the two whenever an exercise adds a hyphenated key to `moonshot.toml`. 
2. When you add a field to `Manifest` or `ProjectMetadata`, the compiler will point you to every place that constructs the struct. The most common one is `src/commands/init.rs`. Later exercises will not always remind you of this; follow the compiler errors.

!!! exercise-easy "Add a `requires-lua` Field"

    Add a top-level `requires-lua` field to `moonshot.toml` (similar to `requires-python` in pyproject.toml). This field is more ergonomic than putting the Lua constraint in `[dependencies]` because it expresses the Lua version as a project-level requirement, not a regular dependency. Parse and validate it through `MatchSpec::from_str`. The `shot init` command gets a `--lua-version` flag to set it.

    /// margin-note
    The value is a version constraint, not a full match spec. Prepend `"lua "` to build a valid spec for `MatchSpec::from_str`. Use a Serde rename so the TOML key keeps its hyphen. See `Manifest::match_specs()` for the parsing pattern.
    ///

    Acceptance criteria
    :   - `shot init --lua-version ">=5.1,<5.5"` writes `requires-lua = ">=5.1,<5.5"` to `[project]`
        - `shot init --lua-version "!!!invalid"` fails with a parse error before creating any file
        - Default (no flag) writes `requires-lua = ">=5.4"`
        - `Manifest` struct has a `requires_lua: Option<String>` field that round-trips through TOML

!!! exercise-intermediate "Detect and Record Virtual Packages"

    At init time, detect the system's virtual packages using `VirtualPackage::detect()` and print them to stdout. Write a `[system]` section into the manifest with the detected values (e.g., `glibc = "2.31"` on Linux, `osx = "15.0"` on macOS). This gives users visibility into what their build host provides, which matters when the project later resolves dependencies (Ch6) or builds platform-specific packages (Ch10).

    /// margin-note
    Call `VirtualPackage::detect` and convert results to `GenericVirtualPackage`. Note that `__archspec` puts the architecture in `build_string`, not `version`. See `src/session.rs` for the detection pattern. Store results as a `HashMap` in the manifest.
    ///

    Acceptance criteria
    :   - `shot init` prints detected virtual packages (e.g., `Detected: __glibc=2.31, __archspec=1=x86_64`)
        - Manifest contains `[system]` with key-value pairs for detected packages
        - On macOS `__osx` is recorded; on Linux `__glibc` is recorded
        - `[system]` is omitted from serialization when empty

!!! exercise-hard "Init with Gateway Validation"

    Add a `--validate` flag to `shot init` that queries the configured channels to verify the Lua version constraint is satisfiable before writing the manifest. This requires constructing an HTTP client, creating a `Gateway`, and querying for a `MatchSpec` matching the `requires-lua` value. If no matching Lua packages exist in the channel, abort with a clear error.

    Dependencies: Exercise 3.1 (uses the `requires-lua` field).

    /// margin-note
    Build an HTTP client (see `src/client.rs`), create a `Gateway`, and query for the lua spec. Follow the gateway pattern in `src/commands/search.rs`.
    ///

    Acceptance criteria
    :   - `shot init --validate` succeeds when `lua >=5.4` exists on conda-forge
        - `shot init --validate --lua-version ">=99.0"` fails with "No Lua packages matching >=99.0 found in channels"
        - Without `--validate`, init works offline as before
        - The channels from `--channel` flags (or the default) are used for the query


In the next chapter we'll implement `shot search`, which queries a channel for
available packages.
