//! # `luapkg install`
//!
//! This is the core of `luapkg` — and the core of any conda-compatible package
//! manager.  It wires together four independent rattler subsystems:
//!
//! 1. **rattler_repodata_gateway** — fetch and cache channel metadata
//! 2. **rattler_virtual_packages** — discover what the host system provides
//! 3. **rattler_solve** — pick a consistent set of package versions
//! 4. **rattler::Installer** — download, extract, and link packages
//!
//! Reading through this file top-to-bottom gives you the complete picture of
//! what happens when you run `conda install` (or `pixi install`, etc.).

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

#[derive(Debug, Parser)]
pub struct Args {
    /// Override the target prefix (where packages are installed).
    ///
    /// Defaults to `.luapkg/env/` relative to the project root.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

/// Shared install logic used by both `install` and `add`.
///
/// Takes a fully-parsed `Manifest` and installs (or updates) the environment
/// at `prefix`.  Pulling this out into its own function means `add` can call
/// it after mutating the manifest without duplicating any networking or
/// solving code.
pub async fn install_from_manifest(
    manifest: &Manifest,
    prefix: std::path::PathBuf,
) -> miette::Result<()> {
    // ── 1. Parse match-specs ────────────────────────────────────────────────
    //
    // A MatchSpec is rattler's name for a package selector: it can contain a
    // name, version constraint, build string, and more.  The full grammar is
    // defined in `rattler_conda_types::match_spec`.
    //
    // We use "strict" mode (no implicit globs) and enable experimental extras
    // (like `[extras=...]` selectors) so that advanced users have access to
    // the full MatchSpec language.
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

    // ── 2. Locate the cache directory ───────────────────────────────────────
    //
    // rattler ships a helper that picks an OS-appropriate cache dir
    // (~/.rattler on Linux/macOS, %APPDATA%\rattler on Windows).  We share
    // this cache with other rattler-based tools (pixi, rattler-build) so
    // repodata and packages are never downloaded twice.
    let cache_dir = rattler::default_cache_dir()
        .map_err(|e| miette::miette!("could not determine cache directory: {e}"))?;
    rattler_cache::ensure_cache_dir(&cache_dir)
        .map_err(|e| miette::miette!("could not create cache directory: {e}"))?;

    // ── 3. Build the HTTP client ─────────────────────────────────────────────
    //
    // We wrap `reqwest::Client` with `reqwest_middleware` so we can layer on:
    //   • AuthenticationMiddleware  — Bearer tokens, Basic auth, keyring lookup
    //   • OciMiddleware             — transparently converts oci:// URLs
    //
    // The authentication middleware reads credentials from:
    //   1. The rattler keyring (populated by `rattler auth`)
    //   2. Standard conda `~/.condarc` tokens
    //   3. Environment variables (CONDA_TOKEN)
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

    // ── 4. Parse channels ────────────────────────────────────────────────────
    //
    // A `Channel` is more than just a URL: it knows whether it is a named
    // channel (e.g. "conda-forge") or an explicit URL, and it can construct
    // all the sub-URLs for different platforms and subdirs.
    let channels: Vec<Channel> = manifest
        .project
        .channels
        .iter()
        .map(|s| Channel::from_str(s, &channel_config))
        .collect::<Result<_, _>>()
        .into_diagnostic()
        .context("parsing channels")?;

    // ── 5. Query repodata via the Gateway ────────────────────────────────────
    //
    // The `Gateway` is rattler's repodata abstraction layer.  Given a list of
    // channels, platforms, and MatchSpecs it will:
    //
    //   a) Fetch `repodata.json` (or the sharded variant) for each
    //      channel/platform combination.
    //   b) Cache the result on disk so subsequent runs are fast.
    //   c) Only load the subset of records that *could* satisfy the specs
    //      (the "sparse repodata" trick — avoids pulling millions of records
    //      into RAM for large channels like conda-forge).
    //   d) Recursively fetch records for transitive dependencies.
    //
    // The call to `.recursive(true)` is the key: it tells the gateway to keep
    // fetching dependency records until the closure is fully explored.
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

    let repo_data: Vec<RepoData> = with_spinner(
        "Fetching repodata",
        gateway
            .query(
                channels,
                [platform, Platform::NoArch], // NoArch covers pure-Lua packages
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

    // ── 6. Detect virtual packages ──────────────────────────────────────────
    //
    // Virtual packages describe capabilities of the *host* system that cannot
    // be installed — things like the OS version, GLIBC version, or CUDA
    // availability.  Packages can express dependencies on virtual packages to
    // ensure they only install on compatible systems.
    //
    // Examples of virtual packages:
    //   __linux          (presence of a Linux kernel)
    //   __glibc >=2.17   (glibc version)
    //   __cuda >=11.0    (CUDA toolkit installed on the host)
    //
    // rattler_virtual_packages does the heavy lifting of probing the system.
    let virtual_packages: Vec<GenericVirtualPackage> =
        rattler_virtual_packages::VirtualPackage::detect(
            &rattler_virtual_packages::VirtualPackageOverrides::default(),
        )
        .into_diagnostic()
        .context("detecting virtual packages")?
        .into_iter()
        .map(|v| v.into())
        .collect();

    // ── 7. Read the currently-installed packages ─────────────────────────────
    //
    // rattler records every installed package in `<prefix>/conda-meta/*.json`
    // (the `PrefixRecord` format).  We pass these to the solver so it can
    // produce a minimal *transaction* (only install/remove what changed) rather
    // than reinstalling everything from scratch each time.
    let installed_packages =
        PrefixRecord::collect_from_prefix::<PrefixRecord>(&prefix).into_diagnostic()?;

    // ── 8. Solve ─────────────────────────────────────────────────────────────
    //
    // The solver receives:
    //   • `available_packages` — records from repodata (what *could* be installed)
    //   • `specs`             — what the user asked for
    //   • `locked_packages`   — what is already installed (try to keep unchanged)
    //   • `virtual_packages`  — host capabilities
    //
    // It returns a flat list of `RepoDataRecord`s — the exact package versions
    // that satisfy all constraints.
    //
    // rattler ships two solver backends:
    //   • `resolvo`    — pure-Rust, the default, used by pixi
    //   • `libsolv_c`  — C binding to libsolv, used by older conda tooling
    //
    // We always use resolvo here.
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

    let start_solve = Instant::now();
    let solution: Vec<RepoDataRecord> = with_spinner_sync("Solving", || {
        resolvo::Solver.solve(solver_task)
    })
    .into_diagnostic()
    .context("solving dependencies")?
    .records;

    println!(
        "  Solved {} packages in {:.1}s",
        console::style(solution.len()).cyan(),
        start_solve.elapsed().as_secs_f64()
    );

    // ── 9. Install ───────────────────────────────────────────────────────────
    //
    // The `Installer` computes a `Transaction` (diff between current and
    // desired state) then executes it:
    //
    //   • Download archives that aren't already in the package cache.
    //   • Extract them into the package cache (one entry per package+hash).
    //   • Hard-link (or copy on systems without hard-link support) files from
    //     the cache into the target prefix.
    //
    // Hard-linking is the key optimisation: a package that is used in ten
    // different environments is stored *once* on disk and linked into each.
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
        println!(
            "  Activate with:  eval $(luapkg shell)"
        );
    }

    Ok(())
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let (_, manifest) = Manifest::find_in_dir(&cwd)?;

    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix)
        .into_diagnostic()
        .context("creating prefix directory")?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    install_from_manifest(&manifest, prefix).await
}

// ── Private helper ────────────────────────────────────────────────────────────

fn with_spinner_sync<T, F: FnOnce() -> T>(
    msg: &'static str,
    f: F,
) -> T {
    crate::progress::with_spinner_sync(msg, f)
}
