# Chapter 7: The Lock File

Every time `shot install` runs, it solves dependencies from scratch. If
conda-forge publishes a new build of `lua` overnight, tomorrow's install might
produce a different environment — even with the same manifest. A **lock file**
fixes this by recording the exact solution so it can be replayed later.

This chapter introduces the `rattler_lock` crate and builds the `src/lock.rs`
module. We don't wire it into `shot install` yet; that integration is left as
an exercise. The goal here is to understand lock files and have the building
blocks ready.

## Concepts

### Why lock files

Without a lock file, the solver picks the best solution *at the time you run
it*. A lock file records the *exact* solution: every package name, version,
build string, and download URL. Replaying the lock gives you the same
environment every time.

Every serious package manager converges on this pattern:

| Tool | Lock file |
|---|---|
| Cargo | `Cargo.lock` |
| npm | `package-lock.json` |
| pip | `requirements.txt` (manual) / `uv.lock` |
| pixi | `pixi.lock` |
| moonshot | `moonshot.lock` |

### Two-phase model

Lock files split the install into two phases:

1. **Resolve** (slow): fetch repodata, run the SAT solver, write the lock.
2. **Install from lock** (fast): read exact packages from the lock, download
   and link them. No solver, no repodata fetch beyond what's cached.

This is the same split Cargo uses: `cargo update` resolves, `cargo build`
installs from the lock.

### Freshness detection

How do we know when to re-solve? We compare file modification times:

- If `moonshot.lock` is **newer** than `moonshot.toml`, the lock is fresh.
  The manifest hasn't changed since the last solve.
- If `moonshot.toml` is **newer** (or the lock doesn't exist), we re-solve.

!!! tip "Content hashing vs mtime"

    pixi uses content hashing for robustness: it hashes the manifest and stores
    the hash in the lock file, so renaming or touching the file doesn't trigger
    a spurious re-solve. mtime comparison is simpler and good enough for a
    teaching project, but be aware of its limitations (e.g., `touch
    moonshot.toml` will force a re-solve even if nothing changed).

### The `rattler_lock` format

`moonshot.lock` is a YAML file following the `rattler_lock` crate's format (the
same format pixi uses for `pixi.lock`). A simplified example:

```yaml
version: 6
environments:
  default:
    channels:
      - url: https://conda.anaconda.org/conda-forge/
    packages:
      osx-arm64:
        - conda: https://conda.anaconda.org/conda-forge/osx-arm64/lua-5.4.7-h5eee18b_0.conda
          ...
      noarch:
        - conda: https://conda.anaconda.org/conda-forge/noarch/luafilesystem-1.8.0-lua_0.conda
          ...
```

Each entry records the exact URL, SHA-256 hash, and full dependency metadata.
The `rattler_lock` crate handles serialization and deserialization.

## Implementation

### `src/lock.rs`

``` {.rust file=src/lock.rs}
#![allow(dead_code)]
<<lock-imports>>

<<lock-filename>>

<<lock-is-fresh>>

<<lock-read>>

<<lock-write>>

<<lock-tests>>
```

#### Imports

``` {.rust #lock-imports}
use std::path::Path;

use miette::{Context, IntoDiagnostic};
use rattler_conda_types::{Channel, Platform, RepoDataRecord};
use rattler_lock::LockFile;
```

#### The lock filename

``` {.rust #lock-filename}
/// The name of the lock file written alongside `moonshot.toml`.
pub const LOCK_FILENAME: &str = "moonshot.lock";
```

#### Freshness check

We compare modification times to decide whether the lock is still valid.

``` {.rust #lock-is-fresh}
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
```

If either file is missing or the OS doesn't support modification times, we
conservatively return `false` (re-solve).

#### Reading a lock file

``` {.rust #lock-read}
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
```

`LockFile::from_path` parses the YAML.  `default_environment()` returns the
`"default"` environment (the only one moonshot uses).
`conda_repodata_records` converts the locked packages back into
`RepoDataRecord`s that the `Installer` understands.

#### Writing a lock file

``` {.rust #lock-write}
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
```

`LockFileBuilder` accumulates packages and channels, deduplicating by content
hash. `finish()` produces a `LockFile` that `to_path` serializes to YAML.

The conversion `record.clone().into()` turns a `RepoDataRecord` into
`rattler_lock::CondaPackageData`, preserving the download URL, hash, and full
dependency metadata.

## Testing

Since the lock module isn't wired into a command yet, we add a unit test to
verify the write → read roundtrip and the freshness check.

``` {.rust #lock-tests}
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
```

Run the tests with `cargo test`:

```console
$ cargo test
running 2 tests
test lock::tests::freshness_check ... ok
test lock::tests::write_then_read_roundtrip ... ok

test result: ok. 2 passed; 0 filtered out
```

## Exercise: integrate locking into `shot install`

With the building blocks from this chapter, you can modify `shot install` to
use the lock file. Here is the high-level approach:

1. **Before solving**, check `is_lock_fresh`. If the lock is fresh, call
   `read_lock_file` to get the solution and skip straight to the `Installer`.
2. **After solving** (when the lock is stale), call `write_lock_file` to
   persist the solution *before* running the `Installer`.
3. Pass the lock path into `install_from_manifest` (or modify `execute`
   directly).

The key insight is that the lock should be written between the solve and the
install: the solver produces the `Vec<RepoDataRecord>`, the lock captures it,
and then the `Installer` consumes it.

## Summary

- A lock file records the exact solution so repeated installs are fast and
  deterministic.
- `is_lock_fresh` compares modification times to decide whether to re-solve.
- `read_lock_file` extracts `RepoDataRecord`s from the lock via `rattler_lock`.
- `write_lock_file` builds a `LockFile` from the solver output and writes YAML.

In the next chapter we implement `shot shell`, which generates a shell
activation script so you can use the installed packages.
