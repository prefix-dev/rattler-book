// ~/~ begin <<book/src/ch04-search.md#src/commands/search.rs>>[init]
// ~/~ begin <<book/src/ch04-search.md#search-imports>>[init]
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
// ~/~ end

// ~/~ begin <<book/src/ch04-search.md#search-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Package name (or prefix) to search for.
    pub query: String,

    /// Channel to search. Defaults to conda-forge.
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,
}
// ~/~ end

// ~/~ begin <<book/src/ch04-search.md#search-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch04-search.md#search-parse-channels>>[init]
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
    // ~/~ end

    // ~/~ begin <<book/src/ch04-search.md#search-http-client>>[init]
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
    // ~/~ end

    // ~/~ begin <<book/src/ch04-search.md#search-gateway>>[init]
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
    // ~/~ end

    // ~/~ begin <<book/src/ch04-search.md#search-query>>[init]
    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(channels, [platform, Platform::NoArch], vec![spec])
            .recursive(false),
    )
    .await
    .into_diagnostic()
    .context("fetching repodata")?;
    // ~/~ end

    // ~/~ begin <<book/src/ch04-search.md#search-results>>[init]
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
    // ~/~ end
}
// ~/~ end
// ~/~ end
