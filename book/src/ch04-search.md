# Chapter 4: The `search` Command

<span class="newthought">Before we can install</span> anything, we need to know what's available. In this chapter
we'll meet repodata, channels, and the [rattler] Gateway by building a search command.

## Design

`shot search <query>` will search for packages matching a name pattern and print
the results:

```console
$ shot search lua
lua          5.4.7   The Lua programming language
luarocks     3.11.1  The Lua package manager
lua-cjson    2.1.0   Fast JSON encoding/decoding for Lua
…
```

The command will accept a `--channel` flag (defaulting to conda-forge) and search
both the current platform and `noarch`.

## Concepts

### What is `repodata`?

Every conda channel serves a file called `repodata.json` for each supported
platform.  It lists every available package with its metadata:

```json
{
  "packages.conda": {
    "lua-5.4.7-h5eee18b_0.conda": {
      "build": "h5eee18b_0",
      "build_number": 0,
      "depends": ["libgcc-ng >=12"],
      "name": "lua",
      "sha256": "abc123...",
      "size": 312449,
      "subdir": "linux-64",
      "version": "5.4.7"
    },
    ...
  }
}
```

For a large channel like conda-forge, this file can be *hundreds* of megabytes.
You really don't want to load all of that into RAM for every command.

Think of repodata as the contract between server and client. The channel publishes it; our package manager consumes it. How this contract is designed determines download speed, caching, and how much work the client has to do before it can even start solving.

### Channels

A `Channel` is more than just a URL string.  It knows whether you gave it a named
channel (`"conda-forge"`) or an explicit URL, and it can construct the sub-URLs
for each platform.

`ChannelConfig` provides the base URL for named channels (by default
`https://conda.anaconda.org/`).  You can override it to point at a local mirror.

### `MatchSpec`s: describing what you want

We've already seen MatchSpecs in the version constraint table from [Chapter 3](ch03-init.md). A few extra things worth knowing about the [CEP-29] syntax:

- `pkg =1.8` (with `=`) is **fuzzy** -- it matches any `1.8.*` release.
- `pkg 1.8` (with a space) is **exact** -- it matches only version `1.8`.
- Bracket syntax lets you filter on any record field: `lua[build_number='>0']`.
- You can pin a channel: `conda-forge::lua >=5.4`.
- It even supports regexes: if a value starts with `^` and ends with `$` it's treated as a regular expression, e.g. `lua[build='^h5ee.*$']`.

[CEP-29]: https://conda.org/learn/ceps/cep-0029/

[rattler] parses a MatchSpec into a typed struct:

```rust
use rattler_conda_types::{MatchSpec, ParseMatchSpecOptions};

let opts = ParseMatchSpecOptions::default();
let spec: MatchSpec = MatchSpec::from_str("lua >=5.4", opts)?;
```

### The `Gateway`

The `Gateway` from [rattler_repodata_gateway] is the main entry point for fetching repodata. It
manages the on-disk cache, the HTTP client, and per-channel configuration.

### Why querying is fast: sharded repodata

The naive approach would be to fetch all of `repodata.json` and load it into RAM. For
conda-forge that's over 350 MB. That's painfully slow on first run and wasteful when
you only care about packages starting with `lua`.

/// margin-note
The full-file approach stopped scaling once conda-forge passed 200,000
package records. Downloading and parsing hundreds of megabytes on every
install was *the* biggest latency bottleneck for conda users. [CEP-16][cep-16] solves this
by splitting repodata into per-package shards so the client never
downloads data it will never read.
///

[CEP-16][cep-16] (sharded repodata) replaces the monolithic file with a
**content-addressed, per-package** scheme:

1. The server publishes a compact **shard index**
   (`repodata_shards.msgpack.zst`, ~670 KB for conda-forge linux-64). It maps
   each package name to the SHA-256 hash of that package's shard file.
2. When you ask for `lua >=5.4`, the client fetches *only the shard for `lua`*
   at `<shards_base_url>/<sha256>.msgpack.zst`.
3. Each shard contains the full metadata (versions, builds, dependencies) for
   that one package name, encoded as zstd-compressed msgpack.
4. The client reads the shard's dependency lists, discovers transitive
   dependencies (like `libgcc-ng`), and fetches those shards in turn.

Because shard URLs are derived from content hashes, the server can serve them
with `Cache-Control: immutable`. An unchanged package keeps the same URL, so
the client never re-downloads shards it already has.

[cep-16]: https://conda.org/learn/ceps/cep-0016/

/// margin-note
Besides CEP-16 sharding, [rattler] also supports downloading the full
`repodata.json` (plain, `.zst`, or `.bz2` compressed) and [JLAP]
incremental patches for updating a cached copy. When a full file is
loaded, rattler parses it lazily (internally called "sparse" loading) so
that only the records you actually query are deserialized.
///

[JLAP]: https://conda.org/learn/ceps/cep-0014/

Setting `sharded_enabled: true` on the Gateway tells it to prefer the sharded
format when available. Both [prefix.dev](https://prefix.dev) and
[anaconda.org](https://anaconda.org) already serve sharded repodata for conda-forge.

/// margin-note
For a detailed look at the networking stack, including reqwest middleware,
authentication, and OCI support, see [Deep Dive: The Networking Stack](deep-dive-networking.md).
///

### The cache directory

[rattler] caches repodata on disk so you don't have to re-download on every run.

`rattler::default_cache_dir()` returns the OS-appropriate location:

- Linux: `~/.cache/rattler/cache`
- macOS: `~/Library/Caches/rattler/cache`
- Windows: `%LOCALAPPDATA%\rattler\cache`

By sharing this cache with [pixi] and [rattler-build], packages are downloaded only
once across all tools. This was a deliberate design choice.

/// margin-note
The cache keys are content hashes, not name-plus-version pairs. This
matters because the same package version can be rebuilt (with a different
build number or build string), and content-addressed keys prevent stale
cache hits when a rebuild produces different files.
///

### The HTTP client

[rattler] uses [reqwest] for HTTP.  We'll build a client with authentication and OCI
support.

## Implementation

### `src/client.rs`: shared HTTP client

Several commands will need an HTTP client with auth and OCI support, so we put
this setup in its own module.

``` {.rust file=src/client.rs}
use std::sync::Arc;

use miette::{Context, IntoDiagnostic};
use rattler_networking::AuthenticationMiddleware;

/// Build an HTTP client with authentication and OCI middleware.
///
/// The returned client handles:
/// - Token/keyring authentication for private channels
/// - `oci://` URL translation for container-registry channels
/// - Disabled automatic gzip (repodata ships pre-compressed)
pub fn build_authenticated_client() -> miette::Result<reqwest_middleware::ClientWithMiddleware> {
    let raw_client = reqwest::Client::builder()
        .no_gzip()
        .build()
        .expect("failed to build HTTP client");

    let client = reqwest_middleware::ClientBuilder::new(raw_client.clone())
        .with_arc(Arc::new(
            AuthenticationMiddleware::from_env_and_defaults()
                .into_diagnostic()
                .context("setting up auth middleware")?,
        ))
        .with(rattler_networking::OciMiddleware::new(raw_client))
        .build();

    Ok(client)
}
```

The `.no_gzip()` call disables reqwest's automatic gzip decompression. Repodata is already served compressed by the channel and rattler handles that decompression itself. Letting reqwest also decompress would interfere.

`reqwest_middleware` wraps `reqwest::Client` to allow pluggable middleware.
Each middleware intercepts every request and response:

- **AuthenticationMiddleware**: injects tokens from the rattler keyring or
  `.condarc`
- **OciMiddleware**: translates `oci://` URLs to the OCI registry
  API so you can use container registries as conda channels

### `src/commands/search.rs`

Let's create a new file for the search command:

``` {.rust file=src/commands/search.rs}
<<search-imports>>
<<search-args>>
<<search-execute>>
```

#### Imports

``` {.rust #search-imports}
use std::collections::HashMap;
use std::env;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{Channel, ChannelConfig, MatchSpec, ParseMatchSpecOptions, Platform};
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};

use crate::client::build_authenticated_client;
use crate::progress::with_spinner;
```

#### Args

``` {.rust #search-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Package name (or prefix) to search for.
    pub query: String,

    /// Channel to search. Defaults to conda-forge.
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,
}
```

#### Execute

The execute function walks through the networking setup we'll reuse in [Chapter 6](ch06-lock.md):
parse channels, build an HTTP client, configure the Gateway, and query repodata.

``` {.rust #search-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
<<search-parse-channels>>
<<search-http-client>>
<<search-gateway>>
<<search-query>>
<<search-results>>
}
```

#### Channel and spec parsing

We convert the `--channel` strings into [rattler] `Channel` objects and parse the
query into a `MatchSpec`.

``` {.rust #search-parse-channels}
    let channel_config =
        ChannelConfig::default_with_root_dir(env::current_dir().into_diagnostic()?);

    let channels: Vec<Channel> = args
        .channel
        .iter()
        .map(|s| Channel::from_str(s, &channel_config))
        .collect::<Result<_, _>>()
        .into_diagnostic()
        .context("parsing channels")?;

    let spec = MatchSpec::from_str(&args.query, ParseMatchSpecOptions::default())
        .into_diagnostic()
        .with_context(|| format!("parsing search query `{}`", args.query))?;

    let cache_dir = rattler::default_cache_dir()
        .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
    rattler_cache::ensure_cache_dir(&cache_dir)
        .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;
```

#### HTTP client

We call the shared helper from `src/client.rs` to build an authenticated HTTP
client. See the section above for the full implementation.

``` {.rust #search-http-client}
    let client = build_authenticated_client()?;
```

#### `Gateway`

The Gateway builder takes the cache directory, HTTP client, and channel
configuration. Setting `sharded_enabled: true` tells it to prefer the fast
sharded format when a channel supports it.

``` {.rust #search-gateway}
    let platform = Platform::current();

    let gateway = Gateway::builder()
        .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
        .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
        .with_client(client)
        .with_channel_config(rattler_repodata_gateway::ChannelConfig {
            default: SourceConfig {
                sharded_enabled: true,
                ..SourceConfig::default()
            },
            per_channel: HashMap::new(),
        })
        .finish();
```

#### Query

`gateway.query(...)` fetches repodata for the requested packages. We set
`.recursive(false)` because we only need direct matches, not transitive
dependencies. We query both the current platform and `NoArch` to cover pure-Lua packages.

``` {.rust #search-query}
    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(channels, [platform, Platform::NoArch], vec![spec])
            .recursive(false),
    )
    .await
    .into_diagnostic()
    .context("fetching repodata")?;
```

#### Result formatting

The query returns a `Vec<RepoData>`, one per channel/platform combination. We
flatten the records, deduplicate by (name, version), and bail early when the
query matched nothing.

``` {.rust #search-results}
    // Collect and deduplicate results by (name, version), keeping the latest.
    let mut seen: HashMap<(String, String), String> = HashMap::new();
    for repo in &repo_data {
        for record in repo.iter() {
            let name = record.package_record.name.as_normalized().to_string();
            let version = record.package_record.version.to_string();
            let key = (name.clone(), version.clone());
            seen.entry(key).or_insert_with(|| name);
        }
    }

    if seen.is_empty() {
        println!("No packages found matching `{}`.", args.query);
        return Ok(());
    }
```

Next we sort alphabetically and show only the latest version per package name.

``` {.rust #search-results}
    // Sort by name, then by version descending.
    let mut results: Vec<(String, String)> = seen.into_keys().collect();
    results.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    // Deduplicate by name (show only latest version per package).
    let mut last_name = String::new();
    let mut count = 0usize;
    for (name, version) in &results {
        if *name == last_name {
            continue;
        }
        last_name.clone_from(name);
        println!("{:<30} {}", console::style(name).cyan(), version);
        count += 1;
    }

    println!("\n{} package(s) found.", count);
    Ok(())
```

### `src/progress.rs`

The progress module provides spinner wrappers we'll reuse in search and install.
A simple spinner does the job for us.

``` {.rust file=src/progress.rs}
use std::borrow::Cow;
use std::future::IntoFuture;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

/// Spinner style shared across the codebase.
pub fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}")
        .unwrap()
        // braille dots feel snappy even at 10 fps
        .tick_strings(&["⠋", "⠙", "⠸", "⠴", "⠦", "⠇", "⠋"])
}
<<with-spinner>>
<<with-spinner-sync>>
```

The async spinner wraps any `IntoFuture`:

``` {.rust #with-spinner}
pub async fn with_spinner<T, F>(msg: impl Into<Cow<'static, str>>, fut: F) -> T
where
    F: IntoFuture<Output = T>,
{
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = fut.into_future().await;
    pb.finish_and_clear();
    result
}
```

### Updates to `src/commands/mod.rs` and `src/main.rs`

The `search` module needs to be registered. We add `pub mod search;` to
`src/commands/mod.rs` (the full file is shown in [Chapter 2](ch02-project-setup.md)) and a `Search`
variant to the `Command` enum in `src/main.rs` (shown in [Chapter 2](ch02-project-setup.md)).

## Running `shot search`

```console
$ shot search lua
⠋ Fetching repodata
lua                            5.4.7
luarocks                       3.11.1
lua-cjson                      2.1.0
luafilesystem                  1.8.0

4 package(s) found.
```

<!-- TODO: Exercises
- Run `shot search python` and count the results. How does this compare to `shot search lua`?
- Try `shot search "lua >=5.4"` with a version constraint. Does the output change?
- Run with `--channel bioconda` instead of conda-forge. What packages show up?
-->

## Summary

- Repodata is a channel's package catalog, and it can be *huge*.
- MatchSpecs describe what packages you're looking for.
- The `Gateway` fetches repodata, preferring the sharded format (CEP-16) when available.
- For search we query with `.recursive(false)` since we only need direct matches.

## Exercises

!!! exercise-easy "Show All Versions with Build Strings"

    Currently `shot search` deduplicates results to show only the latest version per package name. Add a `--all-versions` flag that displays every version found in the repodata. For each version, show the `build` string from `PackageRecord`, giving users visibility into how packages are built.

    /// margin-note
    Each `PackageRecord` has a `build` string and a `version` you can display with `.to_string()`. Adjust the dedup logic in `src/commands/search.rs`.
    ///

    Acceptance criteria
    :   - `shot search lua --all-versions` shows multiple versions (e.g., 5.4.7, 5.4.6, 5.3.5) each with their build string
        - Default behavior (without flag) is unchanged
        - Output format: `lua    5.4.7    h5505292_0`

!!! exercise-intermediate "Display Package Dependencies from Repodata"

    Add a `--deps` flag to `shot search` that prints the dependency list for each matching package. Access `PackageRecord::depends` (a `Vec<String>` of dependency specs) and display each dependency on its own indented line. Parse each dependency back through `MatchSpec::from_str` to validate it and show the structured name + version constraint.

    /// margin-note
    The `depends` field on `PackageRecord` is a list of dependency spec strings. Parse each one with `MatchSpec::from_str` to extract the structured name and constraint.
    ///

    Acceptance criteria
    :   - `shot search lua --deps` shows the latest version of `lua` with its dependencies listed below
        - Each dependency is indented and shows name + constraint (e.g., `  libgcc-ng >=12`)
        - All dependency strings parse through `MatchSpec::from_str` without error
        - Packages with no dependencies show `(no dependencies)`

!!! exercise-hard "Compare Package Versions"

    Implement `shot search <package> --diff <version1> <version2>` that compares two versions of the same package side by side. Query the gateway for both versions, then diff their `PackageRecord` fields: dependencies added/removed/changed, build string, size, and timestamp.

    /// margin-note
    Add `--diff` as a clap arg taking two version strings. The package name comes from the existing `query` arg. Query the gateway twice with pinned specs like `"lua ==5.4.6"`. Compare `PackageRecord` fields between the two results; for dependencies, build a `HashMap` from each version's `depends` list and diff the maps. Note that `timestamp` is `TimestampMs`, not `DateTime`; call `.datetime()` to convert.
    ///

    Acceptance criteria
    :   - `shot search lua --diff 5.4.6 5.4.7` shows differences between the two versions
        - Dependencies diff shows added (+), removed (-), and changed (~) entries
        - Build string, size, and timestamp differences are displayed
        - If either version is not found, a clear error is shown

In the next chapter we'll implement `shot add`, which will let you edit the manifest.
Then in [Chapter 6](ch06-lock.md) we build `shot lock`, which adds solving
to the repodata pipeline and records the result.

[rattler]: https://github.com/conda/rattler
[rattler_repodata_gateway]: https://crates.io/crates/rattler_repodata_gateway
[reqwest]: https://docs.rs/reqwest
[pixi]: https://pixi.sh
[rattler-build]: https://prefix.dev/docs/rattler-build/overview
[indicatif]: https://crates.io/crates/indicatif
