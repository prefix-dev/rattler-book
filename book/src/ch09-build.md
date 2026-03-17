# Chapter 9: Building and Publishing Packages

We've covered installing packages from existing channels.  Now let's close the
loop: building a new package from source and publishing it so others can install
it.

"Closing the loop" means moving from consumer to producer. Up to now, luapkg only consumed packages that someone else built and uploaded. A package manager that can only consume depends on external tooling (like conda-build or rattler-build) to create new packages. By adding a build command, luapkg becomes self-sufficient for the Lua ecosystem: you can write a library, package it, host it on a local channel, and install it with the same tool.

This chapter covers:
- Parsing a `recipe.toml`
- Installing build-time dependencies
- Running a Lua build script
- Packing the result into a `.conda` archive
- Indexing the output directory as a local channel

## The recipe file

```toml
# recipe.toml
[package]
name    = "moonshine"
version = "0.3.0"
license = "MIT"

[source]
path = "."

[channels]
list = ["conda-forge"]

[requirements]
run   = ["lua >=5.4"]
build = ["lua >=5.4"]

[build]
script = "build.lua"
noarch = true
```

The recipe lives in your project directory alongside your source code.

The Rust struct mirrors this structure:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub package:      PackageMeta,
    pub source:       SourceSpec,
    pub channels:     ChannelSpec,
    pub requirements: Requirements,
    pub build:        BuildConfig,
}
```

The `#[serde(default)]` annotations on fields make the entire `[source]`,
`[channels]`, `[requirements]`, and `[build]` sections optional.

## The build script prelude

Writing a build script that manually uses `os.execute("cp ...")` works but is
tedious.  We embed a Lua prelude that provides helper functions:

```rust
const BUILD_PRELUDE: &str = include_str!("../build_prelude.lua");
```

`include_str!` reads a file *at compile time* and bakes it into the binary as a
`&str`.  No file path needed at runtime, no missing-file errors.

The prelude defines helpers like `install_lua(pattern)`,
`install_bin(path)`, `install_share(path, pkg_name)` and sets globals like
`PREFIX`, `SRC_DIR`, etc.  A minimal build script then looks like:

```lua
-- build.lua
install_lua("src/*.lua")
install_bin("scripts/myapp")
```

## Step 1: Create working directories

```rust
let work_dir = tempfile::tempdir()
    .into_diagnostic()
    .context("creating temporary build directory")?;

let build_prefix   = work_dir.path().join("build_prefix");
let install_prefix = work_dir.path().join("install_prefix");
```

We create two directories:

- **`build_prefix`**: where build-time dependencies are installed.  The Lua
  interpreter lives here.  These never appear in the final package.
- **`install_prefix`**: the "fake root" where the build script installs files.
  Everything in here ends up in the package.

`tempfile::tempdir()` creates a temporary directory and returns a `TempDir`
handle.  When the handle is dropped (at the end of `execute`), the directory
is automatically deleted.  This is the RAII pattern: resource cleanup tied to
object lifetime.

The two-prefix design is build isolation. Tools in `build_prefix` (compilers, interpreters, build utilities) are available during the build but never leak into the final package. Without this separation, a build tool could accidentally end up as a runtime dependency, making the package larger and less portable. This is the same principle behind Debian's Build-Depends vs Depends, and it is a key requirement for reproducible builds.

## Step 2: Install build dependencies

```rust
let build_manifest = Manifest {
    project: ProjectMetadata {
        name: format!("{}-build-env", recipe.package.name),
        channels: recipe.channels.list.clone(),
    },
    dependencies: build_deps
        .iter()
        .map(|s| {
            let mut parts = s.splitn(2, ' ');
            let name = parts.next().unwrap_or(s).to_string();
            let spec = parts.next().unwrap_or("*").to_string();
            (name, spec)
        })
        .collect(),
};
install_from_manifest(&build_manifest, build_prefix.clone()).await?;
```

We reuse `install_from_manifest`, the same function `luapkg install` uses.  We
construct a temporary `Manifest` pointing at `build_prefix` instead of the
project's environment.

`"splitn(2, ' ')"` splits the string at most twice: `"lua >=5.4"` becomes
`["lua", ">=5.4"]`.  The `n` in `splitn` is the maximum number of parts, not
splits.

## Step 3: Run the build script

```rust
let lua_bin = find_lua(&build_prefix)?;

let wrapper_src = format!(
    "dofile({prelude:?})\ndofile({script:?})\n",
    prelude = prelude_path.to_string_lossy(),
    script  = script.to_string_lossy(),
);
```

We write a tiny Lua wrapper that loads the prelude then runs the user's script.
Using `{:?}` (debug format) for path strings inserts proper Lua string escaping;
if the path contains backslashes (Windows) or special characters, they'll be
correctly escaped as Lua string literals.

```rust
let status = tokio::process::Command::new(lua_bin)
    .arg(&wrapper_path)
    .env("PREFIX",       install_prefix)
    .env("SRC_DIR",      src_dir)
    .env("BUILD_PREFIX", build_prefix)
    .env("PKG_NAME",     &recipe.package.name)
    .env("PKG_VERSION",  &recipe.package.version)
    .env("PATH",         &new_path)   // includes build_prefix/bin
    .status()
    .await
    .into_diagnostic()?;
```

The build script can use any tool installed in `build_prefix/bin` because we
prepend it to `PATH`.

## Step 4: Write `info/` metadata

Every conda package contains an `info/` directory with metadata.  We need two
files: `index.json` and `paths.json`.

### `info/index.json`

```rust
let index = IndexJson {
    name:         PackageName::from_str(&recipe.package.name)?,
    version:      VersionWithSource::from_str(&recipe.package.version)?,
    build:        recipe.build_string(),
    build_number: recipe.package.build_number,
    noarch:       if recipe.build.noarch { NoArchType::generic() }
                  else { NoArchType::default() },
    depends:      recipe.requirements.run.clone(),
    // ...
};
```

`PackageName` and `VersionWithSource` are rattler's strongly-typed wrappers that
validate their inputs.  `PackageName::from_str("lua 5.4")` returns an error
because spaces aren't allowed in package names; you catch bad recipe data early.

The `noarch` field is a design axis worth understanding. When `noarch` is true (as it is for pure-Lua packages), the package is built once and works on all platforms, stored under the `noarch/` subdirectory. When false, the package is platform-specific and must be built separately for each target. Choosing `noarch` where possible reduces build and hosting costs, but any package containing compiled code or platform-specific paths must be built per-platform.

### `info/paths.json`

```rust
fn collect_paths_json(prefix: &Path) -> miette::Result<PathsJson> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(prefix).into_iter().filter_map(|e| e.ok()) {
        let meta = entry.metadata()?;
        if !meta.is_file() { continue; }

        let rel_path = entry.path()
            .strip_prefix(prefix)?
            .to_path_buf();

        let (sha256, size) = sha256_and_size(entry.path())?;

        entries.push(PathsEntry {
            relative_path: rel_path,
            path_type: PathType::HardLink,
            sha256: Some(sha256),
            size_in_bytes: Some(size),
            ..Default::default()
        });
    }

    Ok(PathsJson { paths: entries, paths_version: 1 })
}
```

`WalkDir` recursively walks a directory tree.  `.filter_map(|e| e.ok())` skips
entries that failed (permission errors, etc.) rather than halting.

The SHA-256 hash is computed with the `sha2` crate:

```rust
fn sha256_and_size(path: &Path) -> miette::Result<(rattler_digest::Sha256Hash, u64)> {
    use std::io::Read;
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];  // 64 KiB buffer
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hasher.finalize(), size))
}
```

We read the file in 64 KiB chunks to avoid loading the entire file into memory.
The `loop` / `break` pattern here is a manual streaming read; `BufReader::read`
returns `0` when the file is exhausted.

## Step 5: Pack into `.conda`

```rust
write_conda_package(
    writer,
    install_prefix,
    &files,
    CompressionLevel::Default,
    None,       // use all CPU threads for zstd
    &out_name,
    Some(&now),
    None,       // no extra progress bar
)?;
```

`rattler_package_streaming::write::write_conda_package` does all the work:
1. Separates `info/` files from payload files.
2. Compresses each group into a `.tar.zst` archive.
3. Wraps both archives and a `metadata.json` into an uncompressed ZIP.

The `.conda` format is designed so that tools can `mmap` the outer ZIP directory
and jump directly to the inner archive they need.

## Step 6: Index the channel

```rust
index_fs(IndexFsConfig {
    channel:         output_dir.clone(),
    target_platform: None,    // discover all subdirs
    write_zst:       true,
    write_shards:    true,
    force:           false,   // incremental
    max_parallel:    4,
    ..Default::default()
})
.await?;
```

After packing, the output directory is not yet a valid conda channel; it has
packages but no `repodata.json`.  `rattler_index::index_fs` scans the directory,
reads every `.conda` file's `info/index.json`, and writes:

- `output/noarch/repodata.json`, the plain JSON catalog
- `output/noarch/repodata.json.zst`, a compressed version
- `output/noarch/repodata_shards.msgpack.zst`, the sharded format

Once indexed, the output directory can be used as a channel directly:

```toml
# Another project's luapkg.toml
[project]
channels = ["./output", "conda-forge"]

[dependencies]
moonshine = ">=0.3"
```

For a production package manager, local indexing is only the first step. You would also need a way to push packages to a remote server, sign them so consumers can verify authenticity, and define a trust model (who is allowed to publish, and how do you revoke a compromised key). These are substantial features that we skip in luapkg, but they are the difference between a local build tool and a real distribution system.

## Summary

- A `recipe.toml` describes how to build a package.
- Build deps are installed into a temporary prefix; run deps go into `info/index.json`.
- `paths.json` lists every file with its SHA-256 hash.
- `write_conda_package` produces the `.conda` archive format.
- `rattler_index` turns the output directory into a valid conda channel.

With `luapkg build` working, our package manager is feature-complete.  In Part II
we'll dive deeper into the underlying mechanisms.
