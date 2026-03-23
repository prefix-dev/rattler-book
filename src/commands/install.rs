// ~/~ begin <<book/src/ch07-install.md#src/commands/install.rs>>[init]
// ~/~ begin <<book/src/ch07-install.md#install-imports>>[init]
use std::env;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler::install::{IndicatifReporter, Installer};
use rattler_conda_types::{
    MatchSpec, ParseMatchSpecOptions, Platform, PrefixRecord, RepoDataRecord,
};
use rattler_networking::AuthenticationMiddleware;

use crate::lock::{is_lock_fresh, read_lock_file, write_lock_file, LOCK_FILENAME};
use crate::manifest::Manifest;
use crate::resolve::{read_locked_packages, resolve_from_manifest};
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-from-manifest>>[init]
/// Shared install logic: resolve from the manifest, then install into `prefix`.
///
/// Pulling this out into its own function means the build command can call
/// it to install build dependencies without duplicating any networking or
/// solving code.
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<Vec<RepoDataRecord>> {
    let (solution, _channels, platform) = resolve_from_manifest(manifest, vec![]).await?;
    let result = solution.clone();
    run_installer(manifest, &prefix, solution, platform).await?;
    Ok(result)
}
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    let lock_path = cwd.join(LOCK_FILENAME);
    let platform = Platform::current();

    let solution = if is_lock_fresh(&lock_path, &manifest_path) {
        read_lock_file(&lock_path, platform)?
    } else {
        let existing = read_locked_packages(&lock_path, platform);
        let (solution, channels, platform) = resolve_from_manifest(&manifest, existing).await?;
        write_lock_file(&lock_path, &channels, platform, &solution)?;
        println!(
            "{} Wrote {} ({} packages)",
            console::style("✔").green(),
            LOCK_FILENAME,
            console::style(solution.len()).cyan()
        );
        solution
    };

    run_installer(&manifest, &prefix, solution, platform).await
}
// ~/~ end

// ~/~ begin <<book/src/ch07-install.md#install-run-installer>>[init]
async fn run_installer(
    manifest: &Manifest,
    prefix: &std::path::Path,
    solution: Vec<RepoDataRecord>,
    platform: Platform,
) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch07-install.md#parse-install-specs>>[init]
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
    // ~/~ begin <<book/src/ch07-install.md#install-client>>[init]
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
    // ~/~ begin <<book/src/ch07-install.md#run-install>>[init]
    let installed_packages =
        PrefixRecord::collect_from_prefix::<PrefixRecord>(prefix).into_diagnostic()?;
    
    let start_install = Instant::now();
    let result = Installer::new()
        .with_download_client(client)
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
        println!("  Activate with:  eval $(shot shell)");
    }
    
    Ok(())
    // ~/~ end
}
// ~/~ end
// ~/~ end
