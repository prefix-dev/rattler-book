//! # `luapkg add`
//!
//! Adds one or more packages to `luapkg.toml` and immediately installs them.
//!
//! ## Example
//!
//! ```text
//! $ luapkg add luarocks "lua >=5.4"
//! ✔ Added 2 package(s) to luapkg.toml
//! [… install output …]
//! ```
//!
//! Package specs follow the conda MatchSpec syntax.  If only a bare name is
//! given (e.g. `luarocks`) the version constraint defaults to `*` (any
//! version).  If a version constraint is included it must be separated from
//! the name by whitespace (e.g. `"lua >=5.4"`) or by `=` (e.g. `lua=5.4`).

use std::env;

use clap::Parser;
use miette::IntoDiagnostic;

use crate::manifest::{Manifest, MANIFEST_FILENAME};

#[derive(Debug, Parser)]
pub struct Args {
    /// Packages to add, e.g. `luarocks` or `"lua >=5.4"`.
    #[clap(required = true)]
    pub packages: Vec<String>,

    /// Override the target prefix.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);
    let (_, mut manifest) = Manifest::find_in_dir(&cwd)?;

    let mut added = 0usize;
    for pkg in &args.packages {
        // Split "name version" or just "name".
        let (name, version) = split_spec(pkg);
        manifest
            .dependencies
            .entry(name.to_string())
            .or_insert_with(|| version.to_string());
        added += 1;
    }

    manifest.write(&manifest_path)?;
    println!(
        "{} Added {added} package(s) to `{MANIFEST_FILENAME}`",
        console::style("✔").green()
    );

    // Install immediately.
    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    std::fs::create_dir_all(&prefix).into_diagnostic()?;
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    super::install::install_from_manifest(&manifest, prefix).await
}

/// Split a spec string like `"lua >=5.4"` into `("lua", ">=5.4")`.
///
/// If no version part is present, the version defaults to `"*"`.
fn split_spec(spec: &str) -> (&str, &str) {
    // Respect quoted strings and handle the common "name version" pattern.
    if let Some(pos) = spec.find(|c: char| c.is_whitespace() || c == '=') {
        let name = spec[..pos].trim();
        let version = spec[pos..].trim().trim_start_matches('=').trim();
        (name, if version.is_empty() { "*" } else { version })
    } else {
        (spec.trim(), "*")
    }
}
