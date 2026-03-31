// ~/~ begin <<book/src/ch03-init.md#src/commands/init.rs>>[init]
// ~/~ begin <<book/src/ch03-init.md#init-imports>>[init]
use std::collections::BTreeMap;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::NamelessMatchSpec;

use crate::manifest::{
    default_platforms, BuildConfig, Manifest, ProjectMetadata, MANIFEST_FILENAME,
};
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#init-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Name of the project.  Defaults to the current directory name.
    pub name: Option<String>,

    /// Conda channels to search (can be repeated).
    #[clap(short, long, default_value = "conda-forge")]
    pub channel: Vec<String>,

    /// Scaffold a library project (adds [build] section and version).
    #[clap(long)]
    pub library: bool,
}
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#init-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);

    if manifest_path.exists() {
        miette::bail!(
            "`{MANIFEST_FILENAME}` already exists in `{}`. \
             Delete it first if you want to re-initialise.",
            cwd.display()
        );
    }

    // Use the supplied name or fall back to the directory name.
    let name = args.name.unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-lua-project")
            .to_string()
    });
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#init-execute>>[1]
    // Build a starter manifest with Lua pre-filled so the user has something
    // to work with immediately.
    let manifest = Manifest {
        project: ProjectMetadata {
            name: name.clone(),
            channels: args.channel,
            platforms: default_platforms(),
            version: if args.library {
                Some("0.1.0".to_string())
            } else {
                None
            },
            license: None,
            description: None,
        },
        dependencies: BTreeMap::from([(
            "lua".to_string(),
            ">=5.4".parse::<NamelessMatchSpec>().unwrap(),
        )]),
        build: if args.library {
            Some(BuildConfig::default())
        } else {
            None
        },
    };
// ~/~ end
// ~/~ begin <<book/src/ch03-init.md#init-execute>>[2]
    manifest.write(&manifest_path)?;

    println!(
        "{} Created `{MANIFEST_FILENAME}` for project \"{name}\"",
        console::style("✔").green()
    );
    if args.library {
        println!("  Build a package with:  shot build");
    }
    println!("  Add packages with:  shot add <package>");
    println!("  Install them with:  shot install");

    Ok(())
}
// ~/~ end
// ~/~ end
