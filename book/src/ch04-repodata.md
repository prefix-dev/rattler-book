# Chapter 4: Fetching Repodata

Before we can install anything, we need to know what exists.  That's what
repodata is for: a channel's catalog of available packages.  In this chapter we
set up the rattler `Gateway` and use it to fetch repodata for our requested
packages.

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
is why rattler ships a "sparse repodata" trick and a sharded format — we'll look
at those at the end of this chapter.

## MatchSpecs: describing what you want

Before we can query repodata, we need to express what we're looking for.  conda
calls this a **MatchSpec**:

```
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

```rust
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

### Rust concept: iterators and `collect`

`HashMap::iter()` returns an iterator over `(&String, &String)` pairs.  `.map()`
transforms each pair using a closure.  `.collect()` gathers the results back into
a collection.

The return type annotation `collect::<miette::Result<_>>()` tells `collect` to
accumulate `miette::Result<MatchSpec>` values into a
`miette::Result<Vec<MatchSpec>>`.  This is a standard Rust pattern: if any element
is an error, the whole `collect` returns that error.  The `_` lets the compiler
infer the inner type (`MatchSpec`).

## The cache directory

rattler caches repodata on disk so it doesn't re-download on every run.

```rust
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

## The HTTP client

rattler uses `reqwest` for HTTP.  We build a client with authentication and OCI
support:

```rust
let raw_client = reqwest::Client::builder()
    .no_gzip()  // repodata is already compressed; we handle it ourselves
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

### `reqwest_middleware` and the middleware pattern

`reqwest_middleware` wraps `reqwest::Client` to allow pluggable middleware.
Middleware intercepts every request and response, allowing:

- **AuthenticationMiddleware**: injects tokens from the rattler keyring or
  `.condarc`
- **OciMiddleware**: transparently translates `oci://` URLs to the OCI registry
  API so you can use container registries as conda channels

This is the same middleware pattern you'll find in web frameworks — a chain of
handlers, each calling `next.run(request)` to pass to the next one.

### `Arc`: sharing ownership

```rust
.with_arc(Arc::new(AuthenticationMiddleware::from_env_and_defaults()?))
```

`Arc<T>` is **atomically reference counted** — a smart pointer that allows
multiple owners.  When you clone an `Arc`, you get a new pointer to the *same*
allocation; the data is freed when the last `Arc` is dropped.

We use `Arc` here because `reqwest_middleware` needs to clone the middleware
internally (to send it to multiple threads), but `AuthenticationMiddleware` may
hold state (credentials) that we don't want to copy.

> **`Arc` vs `Rc`**: `Rc` is the single-threaded version; `Arc` is
> thread-safe.  Since we're using Tokio with multiple threads, we always use
> `Arc`.

## Channels

```rust
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

`ChannelConfig` provides the base URL for named channels — by default
`https://conda.anaconda.org/`.  You can override it with a local mirror.

## The Gateway

Now we arrive at the main actor: `rattler_repodata_gateway::Gateway`.

```rust
let gateway = Gateway::builder()
    .with_cache_dir(cache_dir.join(REPODATA_CACHE_DIR))
    .with_package_cache(PackageCache::new(cache_dir.join(PACKAGE_CACHE_DIR)))
    .with_client(client.clone())
    .with_channel_config(rattler_repodata_gateway::ChannelConfig {
        default: SourceConfig {
            sharded_enabled: true,
            ..SourceConfig::default()
        },
        per_channel: HashMap::new(),
    })
    .finish();
```

The Gateway is a long-lived object — in a real application you'd create it once
and reuse it.  It manages:
- The on-disk repodata cache
- A package cache (for downloaded archives)
- The HTTP client
- Per-channel configuration (sparse vs sharded format)

### Querying the gateway

```rust
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
```

`gateway.query(...)` builds a `Query` object.  `.recursive(true)` makes the
critical difference: instead of only fetching records for the directly-requested
packages, it recursively fetches records for their dependencies, and their
dependencies' dependencies, and so on — until the transitive closure is
complete.

We pass two platforms: `Platform::current()` (e.g., `linux-64`) and
`Platform::NoArch`.  NoArch covers pure-Lua packages that work on every
platform.

The query returns a `Vec<RepoData>`, one entry per channel/platform combination.
Each `RepoData` is a set of `RepoDataRecord`s — enriched package descriptors
that include the download URL alongside the metadata.

## The sparse repodata trick

Why is querying the gateway fast even on the enormous conda-forge channel?

The naive approach fetches all of `repodata.json` and loads it into RAM.  For
conda-forge that file is ~300 MB.  That's slow on the first run and wasteful when
you only need packages starting with `lua`.

The sparse approach works differently:

1. Download a compact **name index** that maps package names to the byte ranges
   in the full repodata where their records live.
2. When you ask for `lua >=5.4`, fetch *only the byte ranges* for `lua` packages
   from the full file.
3. Cache those ranges separately.
4. When transitive deps ask for `libgcc-ng`, fetch only those ranges.

This dramatically reduces both download size and parse time.  The sharded format
goes further: the index is split into small `shard` files, one per package name,
so you only download the shards you need.

rattler supports all three formats (plain JSON, sparse, sharded) transparently.
Setting `sharded_enabled: true` tells it to prefer the sharded format when
available.

## A word on the `with_spinner` helper

```rust
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

This is a small generic wrapper in `src/progress.rs`.  It accepts any type `F`
that implements `IntoFuture` — which includes both `Future`s and types that
convert *into* a `Future` (like `gateway.query(...)` before you `await` it).

The `impl Into<Cow<'static, str>>` bound on `msg` accepts either `&'static str`
or `String` without the caller thinking about it.  `Cow` (**C**lone **o**n
**W**rite) is an enum that's either borrowed (`&str`) or owned (`String`) — it
lets library functions accept both without forcing an allocation.

## Summary

- Repodata is a channel's package catalog; it can be enormous.
- MatchSpecs describe what packages to look for.
- The `Gateway` fetches repodata efficiently using sparse or sharded formats.
- We query with `.recursive(true)` to get all transitive dependencies.
- `Arc` allows multiple owners of shared state across threads.
- `Cow` avoids unnecessary string allocations in generic APIs.

In the next chapter we take the repodata and run it through the solver to pick
the exact versions to install.
