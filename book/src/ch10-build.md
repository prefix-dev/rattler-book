# Chapter 10: The `build` Command

<span class="newthought">A package manager</span> only starts feeling like a real package manager if it can install packages it has actually built itself. Now let's close the
loop: building a new package from source and publishing it so others can install
it.

Up to now, lumen-app has only consumed packages that someone else built.
By adding a build command, we can write our own library (`lumen`, a small Lua library
that wraps ImageMagick), package it, host it on a local channel, and
install it into lumen-app with the same tool.

This is similar to how pyproject.toml works: the
build configuration lives alongside the project manifest. For
moonshot, a single `moonshot.toml` handles both.

## Design

`shot build` reads the `[build]` section from `moonshot.toml`, installs
dependencies into a temporary prefix, runs a Lua build script, packs the result
into a `.conda` archive, and indexes the output directory as a local channel.

We've also designed [rattler-build] for more general purpose package building, it's both
a cli and a library, but I figured that using lua to build is more fun in this case.

[rattler-build]: https://rattler-build.prefix.dev/latest/

```console
$ shot build
Building moonshine 0.3.0 (build lua_0)
  → Installing 2 build dependencies…
  → Running build script `build.lua`
  → Packing 4 files…
  → Indexing channel at /home/user/moonshine/output
✔ Built moonshine-0.3.0-lua_0.conda
  package → /home/user/moonshine/output/noarch/moonshine-0.3.0-lua_0.conda
  channel → /home/user/moonshine/output
```

You can pass `--output-dir` to control where the built `.conda` file lands
(defaults to `./output/`). This directory is then automatically indexed as a conda channel.

Using `shot init` with `--library`, scaffolds a library project:

```console
$ shot init moonshine --library
✔ Created `moonshot.toml` for project "moonshine"
  Build a package with:  shot build
  Add packages with:  shot add <package>
  Install them with:  shot install
```

That creates a `moonshot.toml` with a `[build]` section already filled in:

```toml
[project]
name    = "moonshine"
version = "0.1.0"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"

[build]
script = "build.lua"
noarch = true
build_number = 0
```

The `[build]` section is what distinguishes a library from an
application project. Without it, `shot build` refuses to run. The `[project]`
fields `version`, `license`, and `description` are optional for application
projects but `version` is required when `[build]` is present.

In our simple package manager `[dependencies]` serve a double duty: `shot install` installs them into your
environment, and `shot build` puts them into the package as runtime
requirements. In pixi for example we've actually split these dependency types.

/// margin-note
In a real-world tool you might want a `[dev-dependencies]` section for packages
needed during development (test runners, linters) but that shouldn't ship in
the final package. Moonshot skips this for simplicity.
///

The Rust struct that maps to the `[build]` section lives in `manifest.rs`
alongside `Manifest` and `ProjectMetadata` (which we defined in
[Chapter 3](ch03-init.md)). This block appends to the `manifest-structs`
block from that chapter:

``` {.rust #manifest-structs}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Defaults to "build.lua".
    #[serde(default = "default_script")]
    pub script: String,

    /// `true` for pure Lua packages (the default).
    #[serde(default = "default_noarch")]
    pub noarch: bool,

    /// Increment on rebuilds of the same version.
    #[serde(default)]
    pub build_number: u64,
}

fn default_script() -> String {
    "build.lua".to_string()
}

fn default_noarch() -> bool {
    true
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            script: default_script(),
            noarch: default_noarch(),
            build_number: 0,
        }
    }
}
```

## Concepts

### Build isolation

`shot build` creates two temporary prefixes:

- **build_prefix**: where dependencies (like the Lua interpreter) get installed.
  Tools here are available during the build but never leak into the final
  package.
- **install_prefix**: the "fake root" where the build script installs files.
  Everything in here ends up in the package.

This separation prevents build tools from accidentally becoming runtime
dependencies, making the package larger and less portable. It's the same
principle behind Debian's Build-Depends vs Depends, and it's a requirement for
reproducible builds.

### The `.conda` format

The `.conda` format (version 2) is an uncompressed ZIP containing two inner
`.tar.zst` archives: one for the info metadata and one for the payload files.
[rattler_package_streaming]'s `write_conda_package` function handles creating this
structure.

/// margin-note
For a detailed reference on what's inside a `.conda` archive, including
`info/index.json`, `info/paths.json`, and the outer ZIP structure, see
[Deep Dive: The conda Package Format](deep-dive-package-format.md).
///

### `noarch` packages

When `noarch` is true (as it is for pure-Lua packages or pure Python ones for that matter), 
the package is built once and works on all platforms, stored under the `noarch/` subdirectory. When
false, the package is platform-specific and must be built separately for each
target.

## Implementation

We built `Session::resolve_and_install` in [Chapter 7](ch07-install.md) for
`shot install`. Now we reuse it to install build dependencies into a temporary
prefix. That's one of the payoffs of keeping build configuration in the
manifest.

First, a few helper methods on `Manifest` that derive filenames and paths
from the metadata. These append to the `manifest-impl` block from
[Chapter 3](ch03-init.md):

``` {.rust #manifest-build-helpers}
    /// The build string encoded in the package filename, e.g. `"lua_0"`.
    pub fn build_string(&self) -> String {
        let build_number = self.build.as_ref().map_or(0, |b| b.build_number);
        format!("lua_{}", build_number)
    }

    /// The canonical filename of the output package (without directory).
    ///
    /// e.g. `"moonshine-0.3.0-lua_0.conda"`
    pub fn package_filename(&self) -> miette::Result<String> {
        let version = self.project.version.as_deref().ok_or_else(|| {
            miette::miette!("No `version` in [project]. A version is required to build a package.")
        })?;
        Ok(format!(
            "{}-{}-{}.conda",
            self.project.name,
            version,
            self.build_string()
        ))
    }

    /// The subdirectory where the package should live in a channel.
    ///
    /// Noarch packages go in `noarch/`; platform-specific packages go in
    /// e.g. `linux-64/`.
    pub fn subdir(&self) -> &'static str {
        match &self.build {
            Some(b) if b.noarch => "noarch",
            _ => rattler_conda_types::Platform::current().as_str(),
        }
    }
```

### The `BuildBackend` trait

Before looking at the build command itself, we introduce a `BuildBackend`
trait that encapsulates how build scripts are executed. This lives in
`src/build_backend.rs`:

``` {.rust file=src/build_backend.rs}
<<build-backend-imports>>
<<build-context-struct>>
<<build-backend-trait>>
<<lua-backend-const>>
<<lua-backend-struct>>
<<lua-backend-impl>>
<<lua-find-lua>>
<<lua-run-build-script>>
```

We start with the file-operation imports:

``` {.rust #build-backend-imports}
use std::path::{Path, PathBuf};

use fs_err as fs;
use miette::{Context, IntoDiagnostic};

use crate::manifest::Manifest;
```

The `BuildContext` bundles the paths and metadata a backend needs:

``` {.rust #build-context-struct}
/// Context passed to a [`BuildBackend`] when executing a build.
pub struct BuildContext<'a> {
    pub manifest: &'a Manifest,
    pub src_dir: PathBuf,
    pub install_prefix: PathBuf,
    pub build_prefix: PathBuf,
}
```

The trait itself is generic over the build-script language:

``` {.rust #build-backend-trait}
/// A pluggable build backend.
///
/// Implement this trait to add support for new build-script languages
/// beyond Lua.
#[allow(dead_code)]
pub trait BuildBackend {
    /// Human-readable name of this backend, for log messages.
    fn name(&self) -> &str;

    /// Run the build script, installing files into `ctx.install_prefix`.
    fn run_build(
        &self,
        ctx: &BuildContext<'_>,
    ) -> impl std::future::Future<Output = miette::Result<()>> + Send;
}
```

The trait is generic over the build-script language. Today we only have Lua,
but the design allows adding Python, shell, or other backends later.

Today we only have one backend -- Lua. It loads a prelude script before running the user's build script:

``` {.rust #lua-backend-const}
const BUILD_PRELUDE: &str = include_str!("build_prelude.lua");
```

``` {.rust #lua-backend-struct}
/// The default build backend: runs a Lua build script.
pub struct LuaBuildBackend;
```

``` {.rust #lua-backend-impl}
impl BuildBackend for LuaBuildBackend {
    fn name(&self) -> &str {
        "lua"
    }

    async fn run_build(&self, ctx: &BuildContext<'_>) -> miette::Result<()> {
        let build_config = ctx
            .manifest
            .build
            .as_ref()
            .expect("[build] section validated in execute()");

        let script_path = ctx.src_dir.join(&build_config.script);
        if !script_path.exists() {
            miette::bail!(
                "Build script `{}` not found (expected at `{}`)",
                build_config.script,
                script_path.display()
            );
        }

        let lua_bin = find_lua(&ctx.build_prefix)?;

        println!(
            "  {} Running build script `{}`",
            console::style("→").blue(),
            build_config.script
        );

        run_build_script(
            &lua_bin,
            &script_path,
            &ctx.install_prefix,
            &ctx.src_dir,
            &ctx.build_prefix,
            ctx.manifest,
        )
        .await
    }
}
```

Finding the Lua interpreter requires checking several possible binary names and locations:

``` {.rust #lua-find-lua}
fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
    let bin_dirs: &[&str] = if cfg!(windows) {
        &["Library/bin", "bin"]
    } else {
        &["bin"]
    };
    let exe_ext = if cfg!(windows) { ".exe" } else { "" };

    for bin_dir in bin_dirs {
        let lua = prefix.join(bin_dir).join(format!("lua{exe_ext}"));
        if lua.exists() {
            return Ok(lua);
        }
        for minor in (1u8..=4u8).rev() {
            let versioned = prefix.join(bin_dir).join(format!("lua5.{minor}{exe_ext}"));
            if versioned.exists() {
                return Ok(versioned);
            }
        }
    }

    let searched: Vec<_> = bin_dirs
        .iter()
        .map(|d| prefix.join(d).display().to_string())
        .collect();
    miette::bail!(
        "No Lua interpreter found in `{}`. \
         Add `lua` to [dependencies] in moonshot.toml.",
        searched.join("`, `")
    )
}
```

We locate the Lua interpreter in the build prefix and run the build script
through a wrapper that loads the prelude first. We use a wrapper file rather
than `-e '...'` so that error messages show correct line numbers.

The build script can use any tool installed in `build_prefix/bin` because we
prepend it to `PATH`.

``` {.rust #lua-run-build-script}
async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    let wrapper_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating wrapper temp dir")?;

    let prelude_path = wrapper_dir.path().join("prelude.lua");
    fs::write(&prelude_path, BUILD_PRELUDE)
        .into_diagnostic()
        .context("writing build prelude")?;

    let wrapper_src = format!(
        "dofile({prelude:?})\ndofile({script:?})\n",
        prelude = prelude_path.to_string_lossy(),
        script = script.to_string_lossy(),
    );
    let wrapper_path = wrapper_dir.path().join("wrapper.lua");
    fs::write(&wrapper_path, &wrapper_src)
        .into_diagnostic()
        .context("writing build wrapper")?;
```

With the wrapper file in place, we prepend `build_prefix/bin` to `PATH` so the
build script can call any tool installed as a build dependency. On Windows we
also add `Library/bin`, which is where conda packages place DLLs and executables.

``` {.rust #lua-run-build-script}
    let original_path = std::env::var("PATH").unwrap_or_default();
    let path_sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = if cfg!(windows) {
        format!(
            "{}{path_sep}{}{path_sep}{original_path}",
            build_prefix.join("Library").join("bin").display(),
            build_prefix.join("bin").display(),
        )
    } else {
        format!(
            "{}{path_sep}{original_path}",
            build_prefix.join("bin").display(),
        )
    };

    let build_config = manifest
        .build
        .as_ref()
        .expect("[build] section validated in execute()");

    let status = tokio::process::Command::new(lua_bin)
        .arg(&wrapper_path)
        .env("PREFIX", install_prefix)
        .env("SRC_DIR", src_dir)
        .env("BUILD_PREFIX", build_prefix)
        .env("PKG_NAME", &manifest.project.name)
        .env(
            "PKG_VERSION",
            manifest.project.version.as_deref().unwrap_or("0.0.0"),
        )
        .env("PKG_BUILD_NUM", build_config.build_number.to_string())
        .env("PATH", &new_path)
        .status()
        .await
        .into_diagnostic()
        .context("launching Lua interpreter")?;

    if !status.success() {
        miette::bail!(
            "Build script exited with status {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
```

### The build command

Here is the full file skeleton for `src/commands/build.rs`, with each section
defined as we encounter it:

``` {.rust file=src/commands/build.rs}
<<build-imports>>
<<build-args>>
<<build-execute>>
<<build-write-metadata>>
<<build-collect-paths>>
<<build-sha256>>
<<build-pack-conda>>
```

The build command pulls in packaging, hashing, and indexing crates:

``` {.rust #build-imports}
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Parser;
use fs_err as fs;
use fs_err::File;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_conda_types::package::{IndexJson, PackageFile, PathType, PathsEntry, PathsJson};
use rattler_conda_types::{NoArchType, PackageName, VersionWithSource};
use rattler_index::{index_fs, IndexFsConfig};
use rattler_package_streaming::write::write_conda_package;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::build_backend::{BuildBackend, BuildContext, LuaBuildBackend};
use crate::manifest::{Manifest, MANIFEST_FILENAME};
use crate::project::Project;
use crate::session::Session;
```

An `--output-dir` flag controls where the built package lands:

``` {.rust #build-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Directory where the built `.conda` file is written.
    ///
    /// Defaults to `./output/`.
    #[clap(long, default_value = "output")]
    pub output_dir: PathBuf,
}
```

The build has five stages: discover the project, set up working directories,
install dependencies, run the build script via the backend, and pack the result.

``` {.rust #build-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let session = Session::new(project)?;
    let manifest = &session.project.manifest;
    let cwd = &session.project.root;

    let _build_config = manifest.build.as_ref().ok_or_else(|| {
        miette::miette!(
            "No [build] section in `{MANIFEST_FILENAME}`. \
             Add one to make this project buildable, or run \
             `shot init --library` to start a new library project."
        )
    })?;

    let version = manifest.project.version.as_deref().ok_or_else(|| {
        miette::miette!(
            "No `version` in [project]. \
             A version is required to build a package."
        )
    })?;

    println!(
        "Building {} {} (build {})",
        console::style(&manifest.project.name).cyan(),
        version,
        manifest.build_string(),
    );
```

Next we create the two isolated prefixes in a temporary directory and resolve
the source directory to an absolute path. This keeps all build artifacts out of
the project tree.

``` {.rust #build-execute}
    let work_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating temporary build directory")?;

    let build_prefix = work_dir.path().join("build_prefix");
    let install_prefix = work_dir.path().join("install_prefix");
    fs::create_dir_all(&build_prefix)
        .into_diagnostic()
        .context("creating build_prefix")?;
    fs::create_dir_all(&install_prefix)
        .into_diagnostic()
        .context("creating install_prefix")?;

    let src_dir = std::path::absolute(cwd)
        .into_diagnostic()
        .context("resolving SRC_DIR")?;

    if !manifest.dependencies.is_empty() {
        println!(
            "  {} Installing {} build dependencies…",
            console::style("→").blue(),
            manifest.dependencies.len()
        );
        session.resolve_and_install(build_prefix.clone()).await?;
    }
```

Finally we construct a `BuildContext`, hand it to the Lua backend, and fall
through to packing and indexing.

``` {.rust #build-execute}
    let backend = LuaBuildBackend;
    let ctx = BuildContext {
        manifest,
        src_dir: src_dir.clone(),
        install_prefix: install_prefix.clone(),
        build_prefix: build_prefix.clone(),
    };
    backend.run_build(&ctx).await?;
<<pack-and-index>>
}
```

The execute function now uses `Session` and delegates build-script execution
to the `LuaBuildBackend` via the `BuildBackend` trait. The `BuildContext`
bundles all the paths and metadata the backend needs.

##### Packing and indexing

We write package metadata, pack the `.conda` archive, and index the output
channel so other tools can use it.

``` {.rust #pack-and-index}
    write_package_metadata(&install_prefix, manifest).context("writing package metadata")?;

    let output_dir = std::path::absolute(&args.output_dir)
        .into_diagnostic()
        .context("resolving output directory")?;

    let subdir_dir = output_dir.join(manifest.subdir());
    fs::create_dir_all(&subdir_dir)
        .into_diagnostic()
        .context("creating output subdir")?;

    let filename = manifest.package_filename()?;
    let output_path = subdir_dir.join(&filename);

    pack_conda(&install_prefix, &output_path, manifest)?;
```

With the `.conda` file written, we index the output directory so it becomes a
usable conda channel with a `repodata.json`.

``` {.rust #pack-and-index}
    println!(
        "  {} Indexing channel at {}",
        console::style("→").blue(),
        output_dir.display()
    );
    index_fs(IndexFsConfig {
        channel: output_dir.clone(),
        target_platform: None, // discover all subdirs automatically
        repodata_patch: None,
        write_zst: true,
        write_shards: true,
        force: false, // incremental (only index new packages)
        max_parallel: 4,
        multi_progress: None,
    })
    .await
    .map_err(|e| miette::miette!("{e:#}"))
    .context("indexing output channel")?;

    println!(
        "{} Built {}",
        console::style("✔").green(),
        console::style(&filename).cyan()
    );
    println!("  package → {}", output_path.display());
    println!("  channel → {}", output_dir.display());

    Ok(())
```

### The build script prelude

Writing a build script that manually uses `os.execute("cp ...")` works but is
tedious. So we embed a Lua prelude that provides helper functions. The prelude
runs before every build script, setting up globals (`PREFIX`, `SRC_DIR`,
`BUILD_PREFIX`, `PKG_NAME`, `PKG_VERSION`, `PKG_BUILD_NUM`) and providing
file-system helpers (`mkdir`, `cp`, `install_lua`, `install_bin`).

For the full source and a walkthrough of each function, see
[Deep Dive: The Build Script API](deep-dive-build-script-api.md).

### Packaging

Once the build script finishes, we turn the install prefix into a `.conda`
archive. This involves writing metadata files, collecting file hashes, packing
the archive, and indexing the output channel.

Every conda package contains an `info/` directory with metadata. We need two
files: `index.json` (which the solver reads to understand the package) and
`paths.json` (which lists every file with its checksum).

``` {.rust #build-write-metadata}
fn write_package_metadata(install_prefix: &Path, manifest: &Manifest) -> miette::Result<()> {
<<create-index-json>>
<<write-meta-files>>
}
```

``` {.rust #create-index-json}
    let info_dir = install_prefix.join("info");
    fs::create_dir_all(&info_dir)
        .into_diagnostic()
        .context("creating info/ directory")?;

    let build_config = manifest
        .build
        .as_ref()
        .expect("[build] section validated in execute()");

    let noarch = if build_config.noarch {
        NoArchType::generic()
    } else {
        NoArchType::default()
    };

    let subdir = if build_config.noarch {
        Some("noarch".to_string())
    } else {
        Some(rattler_conda_types::Platform::current().to_string())
    };

    let version_str = manifest.project.version.as_deref().unwrap_or("0.0.0");
```

Now we populate an `IndexJson` struct. This is the metadata the solver reads
when deciding whether a package satisfies a dependency. The identity fields
(name, version, build string) come from the manifest; `depends` lists the
runtime dependencies; and `noarch`/`subdir` control platform targeting.

``` {.rust #create-index-json}
    let index = IndexJson {
        name: PackageName::from_str(&manifest.project.name)
            .into_diagnostic()
            .with_context(|| format!("invalid package name `{}`", manifest.project.name))?,
        version: VersionWithSource::from_str(version_str)
            .into_diagnostic()
            .with_context(|| format!("invalid version `{}`", version_str))?,
        build: manifest.build_string(),
        build_number: build_config.build_number,
        subdir,
        arch: None,
        platform: None,
        noarch,
        depends: manifest.dependency_strings(),
        constrains: vec![],
        experimental_extra_depends: Default::default(),
        features: None,
        license: manifest.project.license.clone(),
        license_family: None,
        purls: None,
        python_site_packages_path: None,
        track_features: vec![],
        timestamp: Some(
            rattler_conda_types::utils::TimestampMs::from_datetime_millis(chrono::Utc::now()),
        ),
    };
```

``` {.rust #write-meta-files}
    let index_path = install_prefix.join(IndexJson::package_path());
    let index_json = serde_json::to_string_pretty(&index)
        .into_diagnostic()
        .context("serializing index.json")?;
    fs::write(&index_path, index_json)
        .into_diagnostic()
        .context("writing info/index.json")?;

    let paths = collect_paths_json(install_prefix).context("building paths.json")?;

    let paths_path = install_prefix.join(PathsJson::package_path());
    let paths_json = serde_json::to_string_pretty(&paths)
        .into_diagnostic()
        .context("serializing paths.json")?;
    fs::write(&paths_path, paths_json)
        .into_diagnostic()
        .context("writing info/paths.json")?;

    Ok(())
```

We walk the install prefix using [walkdir], hash every file with [sha2]'s SHA-256, and record each path
in a `PathsJson` manifest:

``` {.rust #build-collect-paths}
fn collect_paths_json(prefix: &Path) -> miette::Result<PathsJson> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(prefix).into_iter().filter_map(|e| e.ok()) {
        let meta = entry.metadata().into_diagnostic()?;
        if !meta.is_file() {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(prefix)
            .into_diagnostic()
            .context("stripping prefix from path")?
            .to_path_buf();

        // Compute SHA-256 and size for integrity checking.
        let (sha256, size) = sha256_and_size(abs_path)?;

        entries.push(PathsEntry {
            relative_path: rel_path,
            no_link: false,
            path_type: PathType::HardLink,
            prefix_placeholder: None,
            sha256: Some(sha256),
            size_in_bytes: Some(size),
        });
    }

    Ok(PathsJson {
        paths: entries,
        paths_version: 1,
    })
}
```

``` {.rust #build-sha256}
fn sha256_and_size(path: &Path) -> miette::Result<(rattler_digest::Sha256Hash, u64)> {
    use std::io::Read;
    let file = File::open(path)
        .into_diagnostic()
        .with_context(|| format!("opening `{}`", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf).into_diagnostic()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hasher.finalize(), size))
}
```

We pass the install prefix and its file list to `write_conda_package`, which
separates `info/` files from payload files, compresses each group into a
`.tar.zst`, and wraps both into an uncompressed ZIP:

``` {.rust #build-pack-conda}
fn pack_conda(
    install_prefix: &Path,
    output_path: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    // Collect all files relative to the install prefix.
    let files: Vec<PathBuf> = WalkDir::new(install_prefix)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    if files.is_empty() {
        miette::bail!(
            "The build script did not install any files into PREFIX (`{}`). \
             Make sure your build.lua copies files to `os.getenv(\"PREFIX\")`.",
            install_prefix.display()
        );
    }

    println!(
        "  {} Packing {} files…",
        console::style("→").blue(),
        files.len()
    );

    let writer = BufWriter::new(
        File::create(output_path)
            .into_diagnostic()
            .with_context(|| format!("creating output file `{}`", output_path.display()))?,
    );

    let out_name = format!(
        "{}-{}-{}",
        manifest.project.name,
        manifest.project.version.as_deref().unwrap_or("0.0.0"),
        manifest.build_string()
    );

    let now = chrono::Utc::now();
    write_conda_package(
        writer,
        install_prefix,
        &files,
        CompressionLevel::Default,
        None, // use all available CPU threads for zstd
        &out_name,
        Some(&now),
        None, // no progress bar (already shown by our spinner)
    )
    .into_diagnostic()
    .context("writing .conda archive")?;

    Ok(())
}
```

After packing, the output directory isn't yet a valid conda channel. It has
packages but no `repodata.json`. The `index_fs` call inside `execute` scans
the directory, reads every `.conda` file's `info/index.json`, and writes:

- `output/noarch/repodata.json`, the plain JSON catalog
- `output/noarch/repodata.json.zst`, a compressed version
- `output/noarch/repodata_shards.msgpack.zst`, the sharded format

Once indexed, the output directory can be used as a channel:

```toml
# Another project's moonshot.toml
[project]
channels = ["./output", "conda-forge"]

[dependencies]
moonshine = ">=0.3"
```

/// margin-note
For a fully-featured package manager, local indexing is not really the way to go.
Rather you would need a way to push packages to a remote server in some form. 
Luckily if you build on top of conda there are existing hosting options.
///

## Try it

Let's build a package and install it in lumen-app to see the full loop in
action. We'll create `lumen`, a Lua image toolkit that wraps ImageMagick.

First, create a library project:

```console
$ mkdir lumen && cd lumen
$ shot init lumen --library
✔ Created `moonshot.toml` for project "lumen"
```

Edit `moonshot.toml` to add imagemagick as a dependency. Anyone who installs
lumen will get imagemagick automatically:

```toml
[project]
name = "lumen"
version = "0.1.0"
channels = ["conda-forge"]

[dependencies]
lua = ">=5.4"
imagemagick = "*"

[build]
script = "build.lua"
noarch = true
```

Write a Lua module that wraps the `magick` command-line tool. ImageMagick is
a C library with dozens of native dependencies (libpng, libtiff, zlib, and
more). conda-forge provides all of them pre-built; our Lua code just shells
out to `magick`:

```lua
-- lumen.lua
local M = {}

function M.thumbnail(input, size)
    size = size or 128
    local output = input:gsub("(%..+)$", "_thumb%1")
    local cmd = string.format("magick %s -thumbnail %dx%d %s",
        input, size, size, output)
    local ok = os.execute(cmd)
    if not ok then error("magick failed -- is imagemagick installed?") end
    return output
end

function M.grayscale(input)
    local output = input:gsub("(%..+)$", "_gray%1")
    local cmd = string.format("magick %s -colorspace Gray %s", input, output)
    local ok = os.execute(cmd)
    if not ok then error("magick failed -- is imagemagick installed?") end
    return output
end

return M
```

And a build script that installs it:

```lua
-- build.lua
install_lua("lumen.lua")
```

Build the package:

```console
$ shot build
Building lumen 0.1.0 (build lua_0)
  → Installing 2 build dependencies…
  → Running build script `build.lua`
  → Packing 3 files…
  → Indexing channel at /home/user/lumen/output
✔ Built lumen-0.1.0-lua_0.conda
  package → /home/user/lumen/output/noarch/lumen-0.1.0-lua_0.conda
  channel → /home/user/lumen/output
```

Now go back to lumen-app (the project we created in [Chapter 3](ch03-init.md))
and add lumen as a dependency using the local channel:

```console
$ cd ../lumen-app
```

Edit `moonshot.toml`:

```toml
[project]
name = "lumen-app"
channels = ["../lumen/output", "conda-forge"]

[dependencies]
lua = ">=5.4"
lumen = "*"
```

Install and run:

```console
$ shot install
$ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
```

That creates `photo_thumb.jpg`, a 128-pixel thumbnail. ImageMagick and all its
native dependencies were installed by the package manager because lumen declared
them in its manifest.

That's the full loop. The `.conda` file you built is the same format that
conda-forge uses to distribute tens of thousands of packages.

## Exercises

!!! exercise-easy "Inspect Package Contents"

    Add `shot build --inspect <file.conda>` that reads an existing `.conda` package and displays its metadata and file listing. Use `rattler_package_streaming` to read the archive, extract `info/index.json` for metadata and `info/paths.json` for the file list.

    /// margin-note
    Use `stream_conda_info` from `rattler_package_streaming` to read the info section as a tar archive. Look for `IndexJson` and `PathsJson` in `rattler_conda_types::package`. Add `--inspect` as an early-return path in `build.rs`.
    ///

    Acceptance criteria
    :   - `shot build --inspect output/noarch/mypkg-0.1.0-lua_0.conda` prints name, version, build, dependencies
        - A file listing shows all files with their sizes
        - Invalid files produce clear errors

!!! exercise-intermediate "Extended Package Metadata"

    Include `license` and `description` from the manifest in the built package's `IndexJson`, and write an `about.json` file to the package's info directory. Add optional `home` and `dev_url` fields to the manifest's `[project]` section.

    /// margin-note
    The `license` field is already wired up in `build.rs`. Add `home` and `dev_url` to `ProjectMetadata`, then write an `about.json` file alongside `index.json` in the info directory. Define a simple local struct for serialization; rattler's `AboutJson` type uses `Vec<Url>` which is more than you need here.
    ///

    Acceptance criteria
    :   - Built package's `info/index.json` has the `license` field populated
        - An `info/about.json` file exists with description, license, home, dev_url
        - Missing optional fields (e.g., `home` not set in the manifest) are absent from the JSON entirely, not serialized as `null`
        - Verifiable by extracting the .conda

!!! exercise-hard "Build Variants"

    Implement `shot build --variant KEY=VALUE` that produces different packages from the same source with different configurations. Each variant combination gets a unique build string (e.g., `lua54_0` vs `lua51_0`). Variant keys are injected as environment variables during the build script and encoded in the build string and `IndexJson`. Building with `--variant lua=5.4` and `--variant lua=5.1` produces two separate `.conda` packages.

    /// margin-note
    Extend `Manifest::build_string()` to accept variant info. Encode variants into the build string by joining key-value pairs (remove dots: `5.4` becomes `54`). Pass each variant as a `VARIANT_*` env var to the build backend. Files to touch: `src/manifest.rs`, `src/commands/build.rs`, `src/build_backend.rs`.
    ///

    Acceptance criteria
    :   - `shot build --variant lua=5.4` produces a package with build string containing `lua54`
        - `shot build --variant lua=5.1` produces a different package with `lua51` in the build string
        - Multiple variants: `--variant lua=5.4 --variant opt=release` produces `lua54_optrelease_0` (keys sorted alphabetically, values concatenated)
        - Variant keys are available as env vars during build (e.g., `VARIANT_LUA=5.4`)
        - Both packages can coexist in the output directory with separate filenames
        - `rattler_index::index_fs` indexes all variant packages correctly

## Summary

- The `[build]` section in `moonshot.toml` turns a project into a library.
- Dependencies are installed into a temporary build prefix, keeping build tools
  separate from the final package.
- `paths.json` lists every file with its [sha2] SHA-256 hash.
- [rattler_package_streaming]'s `write_conda_package` produces the `.conda` archive format.
- [rattler_index] turns the output directory into a valid conda channel.

With `shot build` working, our package manager is feature-complete! In Part II
we'll look deeper into the mechanisms we've been using.

[rattler_package_streaming]: https://crates.io/crates/rattler_package_streaming
[rattler_index]: https://crates.io/crates/rattler_index
[sha2]: https://crates.io/crates/sha2
[walkdir]: https://crates.io/crates/walkdir
