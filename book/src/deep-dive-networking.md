# Deep Dive: The `rattler` Networking Stack

When you ran `shot search lua` in [Chapter 4](ch04-search.md), the command
printed results in under a second. Behind the scenes, several HTTP requests
fired: the Gateway fetched a shard index from conda-forge, downloaded the
shards for `lua` and related packages, and cached everything locally. Each
request passed through a middleware chain that injected authentication
credentials and translated OCI URLs.

This chapter explains how that stack is assembled and how each piece works.

## `reqwest`: the HTTP client

[reqwest] is the dominant async HTTP client in the Rust ecosystem.  It builds on
[hyper] (a low-level HTTP library) and provides a friendly high-level API.

Key features we rely on:

- **TLS**: we use `rustls-tls` (pure-Rust TLS, no OpenSSL dependency)
- **Connection pooling**: persistent connections are reused across requests
- **Response streaming**: response bodies can be read as streams without
  buffering the whole response

```toml
reqwest = { version = "0.12", default-features = false, features = ["stream"] }
```

The direct `reqwest` dependency only enables `"stream"` (for streaming response
bodies).  TLS is activated through Cargo feature propagation at the crate level:
the project's `[features]` section defines a `rustls-tls` feature that enables
`reqwest/rustls-tls` and `reqwest/rustls-tls-native-roots`, along with the
matching feature on every rattler crate.  This avoids a dependency on OpenSSL,
which complicates static builds and cross-compilation.

### The `no_gzip` flag

```rust
let raw_client = reqwest::Client::builder()
    .no_gzip()
    .build()?;
```

By default reqwest negotiates gzip compression with servers (`Accept-Encoding:
gzip`).  We disable this because repodata files are served pre-compressed as
`.zst` or `.bz2`, and we handle decompression ourselves.  Letting reqwest add a
second layer of compression would be wasteful.

## `reqwest_middleware`: the middleware chain

[reqwest_middleware] wraps `reqwest::Client` with a chain of interceptors:

```rust
let client = reqwest_middleware::ClientBuilder::new(raw_client.clone())
    .with_arc(Arc::new(AuthenticationMiddleware::from_env_and_defaults()?))
    .with(OciMiddleware::new(raw_client))
    .build();
```

Each middleware implements the `Middleware` trait.  Its `handle` method receives
the request and a `next` handle; calling `next.run(req, extensions).await`
passes the request to the next middleware in the chain (or to the actual HTTP
client if this is the last middleware).

## `AuthenticationMiddleware`

Rattler needs to talk to authenticated channels (private conda channels on
Anaconda.org, private S3 buckets, etc.).  The authentication middleware
intercepts every request and injects the appropriate credential.

It reads credentials from, in priority order:

1. **Rattler keyring**: credentials stored by `rattler auth login`
2. **`~/.condarc`**: conda's configuration file, which can contain tokens
3. **Environment variables**: `CONDA_TOKEN`, `CONDA_USER`, `CONDA_PASSWORD`

Different channel types use different auth schemes:

- `conda.anaconda.org`: Bearer token in the URL or `Authorization` header
- Private channels: Basic auth (username + password)
- OCI registries: OAuth2 token exchange

## `OciMiddleware`

[OCI] (Open Container Initiative) registries are Docker image registries, but they
can also store arbitrary blobs, including conda packages.  Using an OCI registry
as a conda channel is becoming more common because it reuses existing container
registry infrastructure.

An OCI channel URL looks like `oci://ghcr.io/my-org/my-channel`.

The `OciMiddleware` transparently intercepts requests to `oci://` URLs and
translates them into OCI registry API calls:

1. Discover the registry's token endpoint.
2. Exchange credentials for a short-lived token.
3. Use the token to fetch blobs from the registry API.

From the perspective of the `Gateway` and the rest of the code, OCI channels
look identical to HTTPS channels.

## S3 and GCS middleware (bonus topic)

rattler also ships middlewares for AWS S3 and Google Cloud Storage.  These are
useful for organizations that host private conda channels in cloud storage buckets.

The S3 middleware uses SigV4 request signing:
```text
Authorization: AWS4-HMAC-SHA256 Credential=AKID/20240301/us-east-1/s3/aws4_request,
               SignedHeaders=host;x-amz-content-sha256;x-amz-date,
               Signature=abc123...
```

The middleware computes the signature from the request content and timestamp, then
injects the `Authorization` header.  The server verifies the signature using the
same shared secret.

## Mirror middleware

Large organizations sometimes run a local mirror of conda-forge to avoid
downloading packages from the internet on every CI run.  The mirror middleware
rewrites request URLs:

```text
https://conda.anaconda.org/conda-forge/linux-64/lua-5.4.7.conda
    -> https://internal-mirror.corp.com/conda-forge/linux-64/lua-5.4.7.conda
```

The mirror is tried first; on failure, the middleware falls through to the original
URL.  This provides transparency (the mirror can be incomplete) while reducing
internet traffic.

## TLS in Rust: rustls vs OpenSSL

Rust has two main TLS implementations:

**OpenSSL (via `openssl-sys`)**: links to the system's OpenSSL.  Widely
compatible but introduces a C dependency, complicates static builds, and causes
security alerts when OpenSSL has vulnerabilities.

**[rustls]**: a pure-Rust TLS 1.2/1.3 implementation.  No C code, no system
dependency, easy to statically link.  It doesn't support some legacy features
(TLS 1.0/1.1, older cipher suites), but that's a feature: rattler doesn't need
to talk to ancient servers.

We use `rustls` via reqwest's `rustls-tls` feature flag.

## Connection reuse and keep-alive

`reqwest::Client` is designed to be cloned and shared.  Cloning doesn't duplicate
the connection pool; it increments a reference count.  All clones share the same
pool.

```rust
let raw_client = reqwest::Client::builder().build()?;
// ... later ...
let middleware_client = ClientBuilder::new(raw_client.clone())
    .with(MyMiddleware)
    .build();
```

Cloning `reqwest::Client` is cheap (it shares the connection pool internally).
We use the raw client for `OciMiddleware` (which internally makes follow-up
requests) and the middleware client for everything else.

The client maintains persistent TCP connections and reuses them across requests
to the same host.  This matters for repodata fetching: hundreds of shard files
from the same server can share a handful of connections.

## Streaming responses for large files

Response bodies are consumed as async streams of byte chunks, so the HTTP
response is never fully loaded into memory.  This is how rattler downloads a
300 MB `repodata.json` without needing 300 MB of RAM.

## Retry middleware

Network requests fail.  Servers go down, DNS hiccups, TCP connections time out.
Reliable tools need automatic retries with exponential backoff.

```toml
reqwest-retry = "0.7"
```

```rust
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

let retry_policy = ExponentialBackoff::builder()
    .retry_bounds(Duration::from_millis(100), Duration::from_secs(30))
    .build_with_max_retries(3);

let client = ClientBuilder::new(raw_client)
    .with(RetryTransientMiddleware::new_with_policy(retry_policy))
    .build();
```

The retry middleware automatically retries on:

- Network errors (connection reset, timeout)
- HTTP 429 Too Many Requests (with `Retry-After` header support)
- HTTP 5xx server errors

It backs off exponentially: 100ms, 200ms, 400ms, then gives up.  This avoids
hammering a struggling server.

## Summary

- `reqwest` provides the async HTTP foundation with rustls TLS.
- `reqwest_middleware` wraps it in a composable middleware chain.
- `AuthenticationMiddleware` injects credentials for private channels.
- `OciMiddleware` translates `oci://` URLs to registry API calls.
- Streaming response bodies avoids loading large files into memory.
[reqwest]: https://docs.rs/reqwest
[hyper]: https://hyper.rs
[reqwest_middleware]: https://docs.rs/reqwest-middleware
[rustls]: https://github.com/rustls/rustls
[OCI]: https://opencontainers.org

- Retry middleware handles transient network failures gracefully.
