#![allow(dead_code)]
// ~/~ begin <<book/src/ch06-lock.md#src/lock.rs>>[init]
// ~/~ begin <<book/src/ch06-lock.md#lock-imports>>[init]
use std::path::Path;

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, Platform, RepoDataRecord};
use rattler_lock::LockFile;
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#lock-filename>>[init]
/// The name of the lock file written alongside `moonshot.toml`.
pub const LOCK_FILENAME: &str = "moonshot.lock";
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#lock-is-fresh>>[init]
/// Returns `true` when the lock file exists and is newer than the manifest.
pub fn is_lock_fresh(lock_path: &Path, manifest_path: &Path) -> bool {
    let (Ok(lock_meta), Ok(manifest_meta)) = (
        std::fs::metadata(lock_path),
        std::fs::metadata(manifest_path),
    ) else {
        return false;
    };
    let (Ok(lock_mtime), Ok(manifest_mtime)) =
        (lock_meta.modified(), manifest_meta.modified())
    else {
        return false;
    };
    lock_mtime >= manifest_mtime
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#lock-read>>[init]
/// Read a lock file and extract the conda records for the given platform.
///
/// Returns the exact packages that were solved last time, ready to be
/// handed to the `Installer`.
pub fn read_lock_file(
    lock_path: &Path,
    platform: Platform,
) -> miette::Result<Vec<RepoDataRecord>> {
    let lock_file = LockFile::from_path(lock_path)
        .into_diagnostic()
        .context("reading lock file")?;

    let env = lock_file
        .default_environment()
        .ok_or_else(|| miette::miette!("lock file has no default environment"))?;

    let records = env
        .conda_repodata_records(platform)
        .into_diagnostic()
        .context("extracting conda records from lock file")?
        .unwrap_or_default();

    Ok(records)
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#lock-write>>[init]
/// Write a lock file containing the solved packages.
///
/// The lock captures the exact solution so that future installs can skip
/// the solver when the manifest hasn't changed.
pub fn write_lock_file(
    lock_path: &Path,
    channels: &[Channel],
    platform: Platform,
    solution: &[RepoDataRecord],
) -> miette::Result<()> {
    let lock_channels: Vec<rattler_lock::Channel> = channels
        .iter()
        .map(|ch| rattler_lock::Channel {
            url: ch.base_url.to_string(),
            used_env_vars: vec![],
        })
        .collect();

    let mut builder = LockFile::builder();
    builder.set_channels("default", lock_channels);

    for record in solution {
        let conda_pkg: rattler_lock::CondaPackageData = record.clone().into();
        builder.add_conda_package("default", platform, conda_pkg);
    }

    let lock_file = builder.finish();
    lock_file
        .to_path(lock_path)
        .into_diagnostic()
        .context("writing lock file")?;

    Ok(())
}
// ~/~ end

// ~/~ begin <<book/src/ch06-lock.md#lock-tests>>[init]
#[cfg(test)]
mod tests {
    use super::*;
    use rattler_conda_types::{
        package::CondaArchiveIdentifier, Channel, ChannelConfig, PackageName, PackageRecord,
        Platform, RepoDataRecord,
    };
    use std::str::FromStr;

    /// Build a minimal `RepoDataRecord` for testing.
    fn dummy_record(name: &str, version: &str) -> RepoDataRecord {
        let channel_config = ChannelConfig::default_with_root_dir(
            std::env::current_dir().expect("cwd"),
        );
        let channel = Channel::from_str("conda-forge", &channel_config).unwrap();
        let mut record = PackageRecord::new(
            PackageName::from_str(name).unwrap(),
            rattler_conda_types::VersionWithSource::from_str(version).unwrap(),
            format!("h0_0"),
        );
        record.subdir = Platform::current().to_string();

        RepoDataRecord {
            package_record: record,
            url: format!(
                "https://conda.anaconda.org/conda-forge/{}/{name}-{version}-h0_0.conda",
                Platform::current()
            )
            .parse()
            .unwrap(),
            channel: Some(channel.name().to_string()),
            identifier: CondaArchiveIdentifier::from_str(
                &format!("{name}-{version}-h0_0.conda"),
            )
            .unwrap()
            .into(),
        }
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(LOCK_FILENAME);
        let channel_config = ChannelConfig::default_with_root_dir(
            std::env::current_dir().expect("cwd"),
        );
        let channels = vec![
            Channel::from_str("conda-forge", &channel_config).unwrap(),
        ];
        let platform = Platform::current();
        let solution = vec![dummy_record("lua", "5.4.7")];

        write_lock_file(&lock_path, &channels, platform, &solution).unwrap();
        assert!(lock_path.exists());

        let records = read_lock_file(&lock_path, platform).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].package_record.name,
            PackageName::from_str("lua").unwrap()
        );
    }

    #[test]
    fn freshness_check() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("moonshot.toml");
        let lock_path = dir.path().join(LOCK_FILENAME);

        // Neither file exists → stale.
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Only manifest exists → stale.
        std::fs::write(&manifest_path, "").unwrap();
        assert!(!is_lock_fresh(&lock_path, &manifest_path));

        // Lock written after manifest → fresh.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&lock_path, "").unwrap();
        assert!(is_lock_fresh(&lock_path, &manifest_path));

        // Manifest touched after lock → stale.
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&manifest_path, "changed").unwrap();
        assert!(!is_lock_fresh(&lock_path, &manifest_path));
    }
}
// ~/~ end
// ~/~ end
