// ~/~ begin <<book/src/ch04-search.md#src/client.rs>>[init]
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
// ~/~ end
