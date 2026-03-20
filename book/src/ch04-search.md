# Chapter 4: The `search` Command

Before we can install anything, we need to know what exists. This chapter
introduces repodata, channels, and the rattler Gateway through a standalone
command that lets the user search for packages by name.

## Design

`shot search <query>` searches for packages matching a name pattern and prints
the results:

```console
$ shot search lua
lua          5.4.7   The Lua programming language
luarocks     3.11.1  The Lua package manager
lua-cjson    2.1.0   Fast JSON encoding/decoding for Lua
…
```

The command accepts a `--channel` flag (defaults to conda-forge) and searches
both the current platform and `noarch`.

## Concepts

### What is repodata?

Every conda channel serves a file called `repodata.json` for each supported
platform.  The file lists every available package with its metadata:

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

For a large channel like conda-forge, this file can be **hundreds of megabytes**.
Loading the whole thing into RAM for every command would be slow.

Repodata is the contract between server and client. The channel publishes it; the package manager consumes it. How you design this contract determines download speed, caching behavior, and how much work the client does before it can start solving.

### Channels

A `Channel` is more than a URL string.  It knows whether the string is a named
channel (`"conda-forge"`) or an explicit URL
(`"https://conda.anaconda.org/conda-forge"`), and it can construct the sub-URLs
for different platforms.

`ChannelConfig` provides the base URL for named channels, by default
`https://conda.anaconda.org/`.  You can override it with a local mirror.

### MatchSpecs: describing what you want

conda calls a package requirement a **MatchSpec**:

```text
lua >=5.4
luarocks *
lua-json =1.3.*
```

A MatchSpec can specify a package name (required), a version constraint
(optional), a build string (optional), a channel (optional), and more.

rattler parses MatchSpecs into a typed struct:

```rust
use rattler_conda_types::{MatchSpec, ParseMatchSpecOptions};

let opts = ParseMatchSpecOptions::default();
let spec: MatchSpec = MatchSpec::from_str("lua >=5.4", opts)?;
```

### The Gateway

`rattler_repodata_gateway::Gateway` is the main type for fetching repodata. It
manages the on-disk repodata cache, a package cache, the HTTP client, and
per-channel configuration (sparse vs sharded format).

### The sparse repodata trick

Why is querying the gateway fast even on the enormous conda-forge channel?

The naive approach fetches all of `repodata.json` and loads it into RAM.  For
conda-forge that file is ~300 MB.  That's slow on the first run and wasteful when
you only need packages starting with `lua`.

!!! info "Why sharding exists"

    The sparse and sharded formats exist because conda-forge outgrew the
    full-file approach. With over 200,000 packages, the full `repodata.json`
    exceeds 300 MB. Downloading and parsing that on every install was the single
    biggest latency bottleneck for conda users. Sharding shifts work to the
    server (pre-splitting repodata by package name) to save the client from
    downloading data it will never read.

The sparse approach works differently:

1. Download a compact **name index** that maps package names to the byte ranges
   in the full repodata where their records live.
2. When you ask for `lua >=5.4`, fetch *only the byte ranges* for `lua` packages
   from the full file.
3. Cache those ranges separately.
4. When transitive deps ask for `libgcc-ng`, fetch only those ranges.

This reduces both download size and parse time.  The sharded format goes further:
the index is split into small `shard` files, one per package name, so you only
download the shards you need.

rattler supports all three formats (plain JSON, sparse, sharded) transparently.
Setting `sharded_enabled: true` tells it to prefer the sharded format when
available.

!!! note "Deep dive"

    For a detailed look at the networking stack, including reqwest middleware,
    authentication, and OCI support, see [Deep Dive: The Networking Stack](deep-dive-networking.md).

### The cache directory

rattler caches repodata on disk so it doesn't re-download on every run.

`rattler::default_cache_dir()` returns the OS-appropriate location:

- Linux/macOS: `~/.rattler/`
- Windows: `%APPDATA%\rattler\`

By sharing this cache with pixi and rattler-build, packages are downloaded only
once across all tools.

!!! tip "Content-addressed caching"

    The cache keys are content hashes, not name-plus-version pairs. This
    matters because the same package version can be rebuilt (with a different
    build number or build string), and content-addressed keys prevent stale
    cache hits when a rebuild produces different files.

### The HTTP client

rattler uses `reqwest` for HTTP.  We build a client with authentication and OCI
support.

The `.no_gzip()` call disables reqwest's automatic gzip decompression. This is a format-level decision: repodata files are already served as `.json.zst` or `.json.bz2` by the channel, and rattler handles that decompression itself. Letting reqwest also decompress would either double-decompress or interfere with rattler's streaming parser.

#### `reqwest_middleware` and the middleware pattern

`reqwest_middleware` wraps `reqwest::Client` to allow pluggable middleware.
Middleware intercepts every request and response, allowing:

- **AuthenticationMiddleware**: injects tokens from the rattler keyring or
  `.condarc`
- **OciMiddleware**: translates `oci://` URLs to the OCI registry
  API so you can use container registries as conda channels

Web frameworks use the same pattern: a chain of handlers, each calling
`next.run(request)` to pass to the next one.

## Implementation

### `src/commands/search.rs`

This file is new. It implements the search command:

``` {.rust file=src/commands/search.rs}
<<search-imports>>

<<search-args>>

<<search-execute>>
```

#### Imports

``` {.rust #search-imports}
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{Channel, ChannelConfig, MatchSpec, ParseMatchSpecOptions, Platform};
use rattler_networking::AuthenticationMiddleware;
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};

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

The execute function walks through the same networking setup that the install
command will reuse in [Chapter 6](ch06-install.md): parse channels, build an HTTP client with
authentication middleware, configure the Gateway, then query repodata.

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

We convert the `--channel` strings into rattler `Channel` objects and parse the
user's query into a `MatchSpec`. The `ChannelConfig` provides the base URL for
named channels (defaulting to `https://conda.anaconda.org/`).

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

We build a `reqwest` client with `.no_gzip()` (repodata is already compressed,
and rattler handles decompression itself), then wrap it in `reqwest_middleware`
with `AuthenticationMiddleware` (for tokens and keyrings) and `OciMiddleware`
(for `oci://` channel URLs).

``` {.rust #search-http-client}
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
```

#### Gateway

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
`.recursive(false)` because search only needs to show what matches the query,
not resolve transitive dependencies. This keeps the fetch fast. We query both
the current platform and `NoArch` to cover pure-Lua packages.

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
flatten the records, deduplicate by (name, version), sort alphabetically, and
print only the latest version per package name.

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

The progress module provides spinner wrappers used by both `search` and
`install`. The full rattler binary uses `indicatif::MultiProgress` with a custom
log writer to prevent tracing output from interleaving with spinners, but for a
teaching project a simple spinner suffices.

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
`src/commands/mod.rs` (the full file is shown in [Chapter 6](ch06-install.md)) and a `Search`
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

## Summary

- Repodata is a channel's package catalog; it can be enormous.
- MatchSpecs describe what packages to look for.
- The `Gateway` fetches repodata using sparse or sharded formats.
- The `search` command queries with `.recursive(false)` since it only needs
  direct matches.

In the next chapter we build on this foundation to implement `shot install`,
which adds solving and installation to the repodata pipeline.
