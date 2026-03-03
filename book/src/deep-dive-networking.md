# Deep Dive: The rattler Networking Stack

rattler uses `reqwest` as its HTTP client, extended with a middleware layer for
authentication, OCI registries, S3/GCS storage, and mirrors.  This chapter
explains how the stack is assembled and how each piece works.

## reqwest: the HTTP client

`reqwest` is the dominant async HTTP client in the Rust ecosystem.  It builds on
`hyper` (a low-level HTTP library) and provides a friendly high-level API.

Key features we rely on:

- **TLS**: we use `rustls-tls` (pure-Rust TLS, no OpenSSL dependency)
- **Connection pooling**: persistent connections are reused across requests
- **Response streaming**: response bodies can be read as streams without
  buffering the whole response

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
```

We explicitly disable the default features and request `rustls-tls` to avoid a
dependency on OpenSSL, which complicates static builds and cross-compilation.

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

`reqwest_middleware` wraps `reqwest::Client` with a chain of interceptors:

```rust
let client = reqwest_middleware::ClientBuilder::new(raw_client.clone())
    .with_arc(Arc::new(AuthenticationMiddleware::from_env_and_defaults()?))
    .with(OciMiddleware::new(raw_client))
    .build();
```

Each middleware implements a trait:

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> Result<Response>;
}
```

`next.run(req, extensions).await` passes the request to the next middleware in
the chain (or to the actual HTTP client if this is the last middleware).

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

OCI (Open Container Initiative) registries are Docker image registries, but they
can also store arbitrary blobs — including conda packages.  Using an OCI registry
as a conda channel is becoming more common as it leverages existing container
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
```
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

```
https://conda.anaconda.org/conda-forge/linux-64/lua-5.4.7.conda
    → https://internal-mirror.corp.com/conda-forge/linux-64/lua-5.4.7.conda
```

The mirror is tried first; on failure, the middleware falls through to the original
URL.  This provides transparency (the mirror can be incomplete) while reducing
internet traffic.

## TLS in Rust: rustls vs OpenSSL

Rust has two main TLS implementations:

**OpenSSL (via `openssl-sys`)**: links to the system's OpenSSL.  Widely
compatible but introduces a C dependency, complicates static builds, and causes
security alerts when OpenSSL has vulnerabilities.

**rustls**: a pure-Rust TLS 1.2/1.3 implementation.  No C code, no system
dependency, easy to statically link.  It doesn't support some legacy features
(TLS 1.0/1.1, older cipher suites), but that's a feature: rattler doesn't need
to talk to ancient servers.

We use `rustls` via reqwest's `rustls-tls` feature flag.

## Connection reuse and keep-alive

`reqwest::Client` is designed to be cloned and shared.  Cloning doesn't duplicate
the connection pool — it increments a reference count.  All clones share the same
pool.

```rust
let raw_client = reqwest::Client::builder().build()?;
// ... later ...
let middleware_client = ClientBuilder::new(raw_client.clone())
    .with(MyMiddleware)
    .build();
```

`raw_client.clone()` is cheap (just an `Arc` clone).  We use the raw client for
`OciMiddleware` (which internally makes follow-up requests) and the middleware
client for everything else.

The client maintains persistent TCP connections and reuses them across requests
to the same host.  This matters for repodata fetching: hundreds of shard files
from the same server can share a handful of connections.

## Streaming responses for large files

```rust
// Conceptually (from rattler_repodata_gateway internals)
let response = client.get(url).send().await?;
let mut stream = response.bytes_stream();

while let Some(chunk) = stream.next().await {
    let bytes = chunk?;
    // process chunk
}
```

`bytes_stream()` returns an async stream of byte chunks.  The HTTP response body
is never fully loaded into memory — we process each chunk as it arrives.  This
is how rattler downloads a 300 MB `repodata.json` without needing 300 MB of RAM.

## Retry middleware

Network requests fail.  Servers go down, DNS hiccups, TCP connections time out.
A production-quality tool needs automatic retries with exponential backoff.

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

It backs off exponentially: 100ms → 200ms → 400ms → give up.  This avoids
hammering a struggling server.

## Summary

- `reqwest` provides the async HTTP foundation with rustls TLS.
- `reqwest_middleware` wraps it in a composable middleware chain.
- `AuthenticationMiddleware` injects credentials for private channels.
- `OciMiddleware` translates `oci://` URLs to registry API calls.
- Streaming response bodies avoids loading large files into memory.
- Retry middleware handles transient network failures gracefully.
