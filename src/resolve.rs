// ~/~ begin <<book/src/ch06-lock.md#src/resolve.rs>>[init]
// ~/~ begin <<book/src/ch06-lock.md#resolve-imports>>[init]
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Instant;

use miette::{Context, IntoDiagnostic};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseMatchSpecOptions, Platform,
    RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::lock::read_lock_file;
use crate::manifest::Manifest;
use crate::progress::with_spinner;
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#resolve-read-locked>>[init]
/// Read existing locked packages from the lock file, if it exists.
///
/// Returns the records from the lock or an empty vector if the file
/// is missing or unreadable. These records can be passed to the solver
/// as `locked_packages` for stable upgrades.
pub fn read_locked_packages(
    lock_path: &std::path::Path,
    platform: Platform,
) -> Vec<RepoDataRecord> {
    if lock_path.exists() {
        read_lock_file(lock_path, platform).unwrap_or_default()
    } else {
        Vec::new()
    }
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#resolve-fn>>[init]
/// Resolve dependencies from a manifest.
///
/// This is the shared resolve pipeline: parse specs, set up the HTTP
/// client and gateway, fetch repodata, detect virtual packages, and
/// run the solver. Both `shot lock` and `shot install` call this.
pub async fn resolve_from_manifest(
    manifest: &Manifest,
    locked_packages: Vec<RepoDataRecord>,
) -> miette::Result<(Vec<RepoDataRecord>, Vec<Channel>, Platform)> {
    // ~/~ begin <<book/src/ch06-lock.md#parse-specs>>[init]
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
    // ~/~ end
    // ~/~ begin <<book/src/ch06-lock.md#setup-client>>[init]
    let cache_dir = rattler::default_cache_dir()
        .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
    rattler_cache::ensure_cache_dir(&cache_dir)
        .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;
    
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
    // ~/~ begin <<book/src/ch06-lock.md#fetch-repodata>>[init]
    let channels: Vec<Channel> = manifest
        .project
        .channels
        .iter()
        .map(|s| Channel::from_str(s, &channel_config))
        .collect::<Result<_, _>>()
        .into_diagnostic()
        .context("parsing channels")?;
    
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
    
    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(
                channels.clone(),
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
    // ~/~ end
    // ~/~ begin <<book/src/ch06-lock.md#run-solver>>[init]
    let virtual_packages: Vec<GenericVirtualPackage> =
        rattler_virtual_packages::VirtualPackage::detect(
            &rattler_virtual_packages::VirtualPackageOverrides::default(),
        )
        .into_diagnostic()
        .context("detecting virtual packages")?
        .into_iter()
        .map(|v| v.into())
        .collect();
    
    let solver_task = SolverTask {
        locked_packages,
        virtual_packages,
        specs,
        ..SolverTask::from_iter(&repo_data)
    };
    
    let start_solve = Instant::now();
    let solution = with_spinner_sync("Solving", || resolvo::Solver.solve(solver_task))
        .into_diagnostic()
        .context("solving dependencies")?
        .records;
    
    println!(
        "  Solved {} packages in {:.1}s",
        console::style(solution.len()).cyan(),
        start_solve.elapsed().as_secs_f64()
    );
    
    Ok((solution, channels, platform))
    // ~/~ end
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#resolve-helpers>>[init]
fn with_spinner_sync<T, F: FnOnce() -> T>(msg: &'static str, f: F) -> T {
    crate::progress::with_spinner_sync(msg, f)
}
// ~/~ end
// ~/~ end
