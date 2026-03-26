// ~/~ begin <<book/src/ch05-add.md#src/commands/add.rs>>[init]
use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::NamelessMatchSpec;

use crate::manifest::MANIFEST_FILENAME;
use crate::project::Project;

#[derive(Debug, Parser)]
pub struct Args {
    /// Packages to add, e.g. `luarocks` or `"lua >=5.4"`.
    #[clap(required = true)]
    pub packages: Vec<String>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let mut project = Project::discover()?;

    // Validate all specs before modifying the manifest.
    let parsed: Vec<(&str, NamelessMatchSpec)> = args
        .packages
        .iter()
        .map(|pkg| {
            let (name, version) = split_spec(pkg);
            let spec: NamelessMatchSpec = version
                .parse()
                .into_diagnostic()
                .with_context(|| format!("invalid dependency spec `{pkg}`"))?;
            Ok((name, spec))
        })
        .collect::<miette::Result<_>>()?;

    let mut added = 0usize;
    for (name, spec) in parsed {
        project
            .manifest
            .dependencies
            .entry(name.to_string())
            .or_insert(spec);
        added += 1;
    }

    project.save()?;
    println!(
        "{} Added {added} package(s) to `{MANIFEST_FILENAME}`",
        console::style("✔").green()
    );
    println!("  Run `shot install` to apply changes.");
    Ok(())
}
// ~/~ begin <<book/src/ch05-add.md#split-spec>>[init]
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
// ~/~ end
// ~/~ end
