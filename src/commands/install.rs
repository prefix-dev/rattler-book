// ~/~ begin <<book/src/ch06-install.md#src/commands/install.rs>>[init]
// ~/~ begin <<book/src/ch06-install.md#install-imports>>[init]
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
// ~/~ end

// ~/~ begin <<book/src/ch06-install.md#install-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
// ~/~ end

// ~/~ begin <<book/src/ch06-install.md#install-from-manifest>>[init]
/// Shared install logic: solve from the manifest, then install into `prefix`.
///
/// Pulling this out into its own function means the build command can call
/// it to install build dependencies without duplicating any networking or
/// solving code.
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch06-install.md#install-parse-specs>>[init]
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

    // ~/~ begin <<book/src/ch06-install.md#install-cache-dir>>[init]
    let cache_dir = rattler::default_cache_dir()
        .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
    rattler_cache::ensure_cache_dir(&cache_dir)
        .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-http-client>>[init]
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
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-parse-channels>>[init]
    let channels: Vec<Channel> = manifest
        .project
        .channels
        .iter()
        .map(|s| Channel::from_str(s, &channel_config))
        .collect::<Result<_, _>>()
        .into_diagnostic()
        .context("parsing channels")?;
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-gateway-builder>>[init]
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
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-gateway-query>>[init]
    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(channels, [platform, Platform::NoArch], specs.clone())
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

    // ~/~ begin <<book/src/ch06-install.md#install-virtual-packages>>[init]
    let virtual_packages: Vec<GenericVirtualPackage> =
        rattler_virtual_packages::VirtualPackage::detect(
            &rattler_virtual_packages::VirtualPackageOverrides::default(),
        )
        .into_diagnostic()
        .context("detecting virtual packages")?
        .into_iter()
        .map(|v| v.into())
        .collect();
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-read-installed>>[init]
    let installed_packages =
        PrefixRecord::collect_from_prefix::<PrefixRecord>(&prefix).into_diagnostic()?;
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-solver-task>>[init]
    let locked = installed_packages
        .iter()
        .map(|r| r.repodata_record.clone())
        .collect::<Vec<_>>();
    
    let solver_task = SolverTask {
        locked_packages: locked,
        virtual_packages,
        specs: specs.clone(),
        ..SolverTask::from_iter(&repo_data)
    };
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-solve>>[init]
    let start_solve = Instant::now();
    let solution: Vec<RepoDataRecord> =
        with_spinner_sync("Solving", || resolvo::Solver.solve(solver_task))
            .into_diagnostic()
            .context("solving dependencies")?
            .records;
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-solve-progress>>[init]
    println!(
        "  Solved {} packages in {:.1}s",
        console::style(solution.len()).cyan(),
        start_solve.elapsed().as_secs_f64()
    );
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-installer>>[init]
    let start_install = Instant::now();
    let result = Installer::new()
        .with_download_client(client)
        .with_target_platform(platform)
        .with_installed_packages(installed_packages)
        .with_execute_link_scripts(true)
        .with_requested_specs(specs)
        .with_reporter(IndicatifReporter::builder().finish())
        .install(&prefix, solution)
        .await
        .into_diagnostic()
        .context("installing packages")?;
    // ~/~ end

    // ~/~ begin <<book/src/ch06-install.md#install-result>>[init]
    if result.transaction.operations.is_empty() {
        println!(
            "{} Environment already up to date",
            console::style("✔").green()
        );
    } else {
        println!(
            "{} Environment updated in {:.1}s",
            console::style("✔").green(),
            start_install.elapsed().as_secs_f64()
        );
        println!("  Activate with:  eval $(shot shell)");
    }
    
    Ok(())
    // ~/~ end
}
// ~/~ end

// ~/~ begin <<book/src/ch06-install.md#install-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (_, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    install_from_manifest(&manifest, prefix).await
}
// ~/~ end

// ~/~ begin <<book/src/ch06-install.md#install-private-helpers>>[init]
fn with_spinner_sync<T, F: FnOnce() -> T>(msg: &'static str, f: F) -> T {
    crate::progress::with_spinner_sync(msg, f)
}
// ~/~ end
// ~/~ end
