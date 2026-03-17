# Chapter 4: Fetching Repodata

Before we can install anything, we need to know what exists.  That's what
repodata is for: a channel's catalog of available packages.  In this chapter we
set up the rattler `Gateway` and use it to fetch repodata for our requested
packages.

Repodata is the contract between server and client. The channel publishes it; the package manager consumes it. How you design this contract determines download speed, caching behavior, and how much work the client does before it can even start solving.

## What is repodata?

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
Loading the whole thing into RAM for every `luapkg install` would be slow.  This
is why rattler ships a "sparse repodata" trick and a sharded format; we'll look
at those at the end of this chapter.

The install command is the core of luapkg. Its implementation spans this chapter
(repodata fetching), Chapter 5 (solving), and Chapter 6 (installing). Here is
the full file skeleton, with each section defined as we encounter it:

``` {.rust file=src/commands/install.rs}
<<install-imports>>

<<install-args>>

<<install-from-manifest>>

<<install-execute>>

<<install-private-helpers>>
```

``` {.rust #install-imports}
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseMatchSpecOptions, Platform,
    PrefixRecord, RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::manifest::Manifest;
use crate::progress::with_spinner;
```

``` {.rust #install-args}
#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.luapkg/env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
```

## MatchSpecs: describing what you want

Before we can query repodata, we need to express what we're looking for.  conda
calls this a **MatchSpec**:

```text
lua >=5.4
luarocks *
lua-json =1.3.*
```

A MatchSpec can specify:
- A package name (required)
- A version constraint (optional)
- A build string (optional)
- A channel (optional)
- And more

rattler parses MatchSpecs into a typed struct:

```rust
use rattler_conda_types::{MatchSpec, ParseMatchSpecOptions};

let opts = ParseMatchSpecOptions::default();
let spec: MatchSpec = MatchSpec::from_str("lua >=5.4", opts)?;
```

In `luapkg`, we parse MatchSpecs from the manifest's `[dependencies]` table:

``` {.rust #install-parse-specs}
    let channel_config =
        ChannelConfig::default_with_root_dir(env::current_dir().into_diagnostic()?);

    let match_spec_opts = ParseMatchSpecOptions::default();
    let specs: Vec<MatchSpec> = manifest
        .dependencies
        .iter()
        .map(|(name, version)| {
            let spec_str = if version == "*" {
                name.clone()
            } else {
                format!("{name} {version}")
            };
            MatchSpec::from_str(&spec_str, match_spec_opts)
                .into_diagnostic()
                .with_context(|| format!("parsing spec `{spec_str}`"))
        })
        .collect::<miette::Result<_>>()?;
```

## The cache directory

rattler caches repodata on disk so it doesn't re-download on every run.

``` {.rust #install-cache-dir}
    let cache_dir = rattler::default_cache_dir()
        .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
    rattler_cache::ensure_cache_dir(&cache_dir)
        .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;
```

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

## The HTTP client

rattler uses `reqwest` for HTTP.  We build a client with authentication and OCI
support.  Credentials can also come from the `CONDA_TOKEN` environment variable.

``` {.rust #install-http-client}
    let raw_client = reqwest::Client::builder()
        .no_gzip() // repodata is already compressed; we handle it ourselves
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

The `.no_gzip()` call disables reqwest's automatic gzip decompression. This is a format-level decision: repodata files are already served as `.json.zst` or `.json.bz2` by the channel, and rattler handles that decompression itself. Letting reqwest also decompress would either double-decompress or interfere with rattler's streaming parser.

### `reqwest_middleware` and the middleware pattern

`reqwest_middleware` wraps `reqwest::Client` to allow pluggable middleware.
Middleware intercepts every request and response, allowing:

- **AuthenticationMiddleware**: injects tokens from the rattler keyring or
  `.condarc`
- **OciMiddleware**: transparently translates `oci://` URLs to the OCI registry
  API so you can use container registries as conda channels

Web frameworks use the same pattern: a chain of handlers, each calling
`next.run(request)` to pass to the next one.

## Channels

We parse the channel strings from the manifest into rattler's `Channel` type.

``` {.rust #install-parse-channels}
    let channels: Vec<Channel> = manifest
        .project
        .channels
        .iter()
        .map(|s| Channel::from_str(s, &channel_config))
        .collect::<Result<_, _>>()
        .into_diagnostic()
        .context("parsing channels")?;
```

A `Channel` is more than a URL string.  It knows whether the string is a named
channel (`"conda-forge"`) or an explicit URL
(`"https://conda.anaconda.org/conda-forge"`), and it can construct the sub-URLs
for different platforms.

`ChannelConfig` provides the base URL for named channels, by default
`https://conda.anaconda.org/`.  You can override it with a local mirror.

## The Gateway

The main type here is `rattler_repodata_gateway::Gateway`.

``` {.rust #install-gateway-builder}
    let platform = Platform::current();

    let gateway = Gateway::builder()
        .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
        .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
        .with_client(client.clone())
        .with_channel_config(rattler_repodata_gateway::ChannelConfig {
            default: SourceConfig {
                sharded_enabled: true, // prefer the fast sharded format
                ..SourceConfig::default()
            },
            per_channel: HashMap::new(),
        })
        .finish();
```

The Gateway is a long-lived object; in a real application you'd create it once
and reuse it.  It manages:
- The on-disk repodata cache
- A package cache (for downloaded archives)
- The HTTP client
- Per-channel configuration (sparse vs sharded format)

### Querying the gateway

With the gateway configured, we can fetch the repodata for our requested packages.

``` {.rust #install-gateway-query}
    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(
                channels,
                [platform, Platform::NoArch],
                specs.clone(),
            )
            .recursive(true),
    )
    .await
    .into_diagnostic()
    .context("fetching repodata")?;

    let total_records: usize = repo_data.iter().map(RepoData::len).sum();
    println!(
        "  {} repodata records loaded",
        console::style(total_records).cyan()
    );
```

`gateway.query(...)` builds a `Query` object.  `.recursive(true)` makes the
critical difference: instead of only fetching records for the directly-requested
packages, it recursively fetches records for their dependencies, and their
dependencies' dependencies, until the transitive closure is complete.

We pass two platforms: `Platform::current()` (e.g., `linux-64`) and
`Platform::NoArch`.  NoArch covers pure-Lua packages that work on every
platform.

The query returns a `Vec<RepoData>`, one entry per channel/platform combination.
Each `RepoData` is a set of `RepoDataRecord`s, enriched package descriptors
that include the download URL alongside the metadata.

## The sparse repodata trick

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

## A word on the `with_spinner` helper

The full `src/progress.rs` module is small enough to show in its entirety. It
provides two spinner wrappers (async and sync) and a shared style. The full
rattler binary uses `indicatif::MultiProgress` with a custom log writer to
prevent tracing output from interleaving with spinners, but for a teaching
project a simple spinner suffices.

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

This is a small generic wrapper in `src/progress.rs` that accepts any
`IntoFuture` (like `gateway.query(...)` before you `await` it).

## Summary

- Repodata is a channel's package catalog; it can be enormous.
- MatchSpecs describe what packages to look for.
- The `Gateway` fetches repodata efficiently using sparse or sharded formats.
- We query with `.recursive(true)` to get all transitive dependencies.

In the next chapter we take the repodata and run it through the solver to pick
the exact versions to install.
