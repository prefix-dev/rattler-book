// ~/~ begin <<book/src/ch05-add.md#src/commands/add.rs>>[init]
// ~/~ begin <<book/src/ch05-add.md#add-imports>>[init]
use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::NamelessMatchSpec;

use crate::manifest::MANIFEST_FILENAME;
use crate::project::Project;
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#add-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Packages to add, e.g. `luarocks` or `"lua >=5.4"`.
    #[clap(required = true)]
    pub packages: Vec<String>,
}
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#add-execute>>[init]
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
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#add-execute>>[1]
    let mut added = 0usize;
    for (name, spec) in parsed {
        let len_before = project.manifest.dependencies.len();
        project
            .manifest
            .dependencies
            .entry(name.to_string())
            .or_insert(spec);
        if project.manifest.dependencies.len() > len_before {
            added += 1;
        }
    }

    project.save()?;
    println!(
        "{} Added {added} package(s) to `{MANIFEST_FILENAME}`",
        console::style("✔").green()
    );
    println!("  Run `shot install` to apply changes.");
    Ok(())
}
// ~/~ end
// ~/~ begin <<book/src/ch05-add.md#split-spec>>[init]
fn split_spec(spec: &str) -> (&str, &str) {
    // Split on first whitespace only; operator-prefixed versions require a space.
    if let Some(pos) = spec.find(char::is_whitespace) {
        let name = spec[..pos].trim();
        let version = spec[pos..].trim();
        (name, if version.is_empty() { "*" } else { version })
    } else {
        (spec.trim(), "*")
    }
}
// ~/~ end
// ~/~ end
