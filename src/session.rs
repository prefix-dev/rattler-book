// ~/~ begin <<book/src/ch06-lock.md#src/session.rs>>[init]
// ~/~ begin <<book/src/ch06-lock.md#session-imports>>[init]
use std::collections::HashMap;
use std::time::Instant;

use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler::package_cache::PackageCache;
use rattler_cache::{PACKAGE_CACHE_DIR, REPODATA_CACHE_DIR};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_repodata_gateway::{Gateway, RepoData, SourceConfig};
use rattler_solve::{resolvo, SolverImpl, SolverTask};

use crate::client::build_authenticated_client;
use crate::lock::{read_lock_file, read_locked_packages, write_lock_file};
use crate::progress::{with_spinner, with_spinner_sync};
use crate::project::Project;
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#resolve-status-enum>>[init]
/// The outcome of [`Session::ensure_resolved`].
pub enum ResolveStatus {
    /// The lock file was already up to date; no work was done.
    AlreadyFresh(Vec<RepoDataRecord>),
    /// A fresh solve was performed and the lock file was written.
    Resolved {
        solution: Vec<RepoDataRecord>,
        #[allow(dead_code)]
        platform: Platform,
    },
}

#[allow(dead_code)]
impl ResolveStatus {
    /// Extract the solution regardless of which variant we are.
    pub fn solution(&self) -> &[RepoDataRecord] {
        match self {
            ResolveStatus::AlreadyFresh(s) => s,
            ResolveStatus::Resolved { solution, .. } => solution,
        }
    }

    pub fn into_solution(self) -> Vec<RepoDataRecord> {
        match self {
            ResolveStatus::AlreadyFresh(s) => s,
            ResolveStatus::Resolved { solution, .. } => solution,
        }
    }
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#session-struct>>[init]
/// Bundles a [`Project`] with an HTTP client and repodata gateway.
#[allow(dead_code)]
pub struct Session {
    pub project: Project,
    pub client: reqwest_middleware::ClientWithMiddleware,
    pub gateway: Gateway,
    pub cache_dir: std::path::PathBuf,
    pub channel_config: ChannelConfig,
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#session-new>>[init]
impl Session {
    /// Create a new session from a discovered project.
    pub fn new(project: Project) -> miette::Result<Self> {
        let cache_dir = rattler::default_cache_dir()
            .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
        rattler_cache::ensure_cache_dir(&cache_dir)
            .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;

        let client = build_authenticated_client()?;
        let channel_config = ChannelConfig::default_with_root_dir(project.root.clone());

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

        Ok(Self {
            project,
            client,
            gateway,
            cache_dir,
            channel_config,
        })
    }
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#session-channels>>[init]
    /// Parse the manifest channels into typed [`Channel`] values.
    pub fn channels(&self) -> miette::Result<Vec<Channel>> {
        self.project
            .manifest
            .project
            .channels
            .iter()
            .map(|s| Channel::from_str(s, &self.channel_config))
            .collect::<Result<_, _>>()
            .into_diagnostic()
            .context("parsing channels")
    }
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#session-resolve>>[init]
    /// Run the full dependency-resolution pipeline.
    pub async fn resolve(
        &self,
        locked_packages: Vec<RepoDataRecord>,
    ) -> miette::Result<(Vec<RepoDataRecord>, Vec<Channel>, Platform)> {
        let specs = self.project.manifest.match_specs()?;
        let channels = self.channels()?;
        let platform = Platform::current();

        let repo_data: Vec<RepoData> = with_spinner(
            "Fetching repodata",
            self.gateway
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
    }
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#session-ensure-resolved>>[init]
    /// Ensure the lock file is up to date, resolving if necessary.
    pub async fn ensure_resolved(&self, force: bool) -> miette::Result<ResolveStatus> {
        let lock_path = self.project.lock_path();
        let platform = Platform::current();

        if !force && self.project.is_lock_fresh() {
            let records = read_lock_file(&lock_path, platform)?;
            return Ok(ResolveStatus::AlreadyFresh(records));
        }

        let existing = read_locked_packages(&lock_path, platform);
        let (solution, channels, platform) = self.resolve(existing).await?;

        write_lock_file(&lock_path, &channels, platform, &solution)?;

        Ok(ResolveStatus::Resolved { solution, platform })
    }
}
// ~/~ end
// ~/~ end
// ~/~ begin <<book/src/ch07-install.md#src/session.rs>>[0]
// ~/~ begin <<book/src/ch07-install.md#session-install-packages>>[init]
impl Session {
    /// Install a set of solved packages into the given prefix.
    pub async fn install_packages(
        &self,
        prefix: &std::path::Path,
        solution: Vec<RepoDataRecord>,
        platform: Platform,
    ) -> miette::Result<()> {
        let specs = self.project.manifest.match_specs()?;

        let installed_packages =
            PrefixRecord::collect_from_prefix::<PrefixRecord>(prefix).into_diagnostic()?;

        let start_install = Instant::now();
        let result = Installer::new()
            .with_download_client(self.client.clone())
            .with_target_platform(platform)
            .with_installed_packages(installed_packages)
            .with_execute_link_scripts(true)
            .with_requested_specs(specs)
            .with_reporter(IndicatifReporter::builder().finish())
            .install(prefix, solution)
            .await
            .into_diagnostic()
            .context("installing packages")?;

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
            println!("  Activate with:  eval $(shot shell-hook)");
        }

        Ok(())
    }
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#session-resolve-and-install>>[init]
    /// Resolve and install in one step, without writing a lock file.
    pub async fn resolve_and_install(
        &self,
        prefix: std::path::PathBuf,
    ) -> miette::Result<Vec<RepoDataRecord>> {
        let (solution, _channels, platform) = self.resolve(vec![]).await?;
        let result = solution.clone();
        self.install_packages(&prefix, solution, platform).await?;
        Ok(result)
    }
}
// ~/~ end
// ~/~ end
