// ~/~ begin <<book/src/ch09-build.md#src/commands/build.rs>>[init]
use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_conda_types::package::{IndexJson, PackageFile, PathType, PathsEntry, PathsJson};
use rattler_conda_types::{NoArchType, PackageName, VersionWithSource};
use rattler_index::{index_fs, IndexFsConfig};
use rattler_package_streaming::write::write_conda_package;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::commands::install::install_from_manifest;
use crate::manifest::{Manifest, ProjectMetadata};
use crate::recipe::{Recipe, RECIPE_FILENAME};

#[derive(Debug, Parser)]
pub struct Args {
    /// Path to `recipe.toml`.  Defaults to `./recipe.toml`.
    #[clap(long)]
    pub recipe: Option<PathBuf>,

    /// Directory where the built `.conda` file is written.
    ///
    /// Defaults to `./output/`.
    #[clap(long, default_value = "output")]
    pub output_dir: PathBuf,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;

    let recipe_path = args
        .recipe
        .clone()
        .unwrap_or_else(|| cwd.join(RECIPE_FILENAME));

    let recipe = Recipe::from_path(&recipe_path)?;
    let recipe_dir = recipe_path.parent().unwrap_or(&cwd).to_path_buf();

    println!(
        "Building {} {} (build {})",
        console::style(&recipe.package.name).cyan(),
        recipe.package.version,
        recipe.build_string(),
    );

    let work_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating temporary build directory")?;

    let build_prefix = work_dir.path().join("build_prefix");
    let install_prefix = work_dir.path().join("install_prefix");
    std::fs::create_dir_all(&build_prefix)
        .into_diagnostic()
        .context("creating build_prefix")?;
    std::fs::create_dir_all(&install_prefix)
        .into_diagnostic()
        .context("creating install_prefix")?;

    // Resolve the source directory.
    let src_dir = {
        let p = PathBuf::from(&recipe.source.path);
        if p.is_absolute() {
            p
        } else {
            recipe_dir.join(&recipe.source.path)
        }
    };
    let src_dir = std::path::absolute(src_dir)
        .into_diagnostic()
        .context("resolving SRC_DIR")?;

    let mut build_deps = recipe.requirements.build.clone();
    // Always ensure lua is available in the build environment.
    if !build_deps.iter().any(|d| d.starts_with("lua")) {
        build_deps.push("lua >=5.1".to_string());
    }

    if !build_deps.is_empty() {
        println!(
            "  {} Installing {} build dependencies…",
            console::style("→").blue(),
            build_deps.len()
        );
        let build_manifest = Manifest {
            project: ProjectMetadata {
                name: format!("{}-build-env", recipe.package.name),
                channels: recipe.channels.list.clone(),
            },
            dependencies: build_deps
                .iter()
                .map(|s| {
                    // Split "name version" into (name, spec) pair
                    let mut parts = s.splitn(2, ' ');
                    let name = parts.next().unwrap_or(s).to_string();
                    let spec = parts.next().unwrap_or("*").to_string();
                    (name, spec)
                })
                .collect(),
        };
        install_from_manifest(&build_manifest, build_prefix.clone()).await?;
    }

    let script_path = recipe_dir.join(&recipe.build.script);
    if !script_path.exists() {
        miette::bail!(
            "Build script `{}` not found (expected at `{}`)",
            recipe.build.script,
            script_path.display()
        );
    }

    let lua_bin = find_lua(&build_prefix)?;

    println!(
        "  {} Running build script `{}`",
        console::style("→").blue(),
        recipe.build.script
    );

    run_build_script(
        &lua_bin,
        &script_path,
        &install_prefix,
        &src_dir,
        &build_prefix,
        &recipe,
    )
    .await?;

    write_package_metadata(&install_prefix, &recipe).context("writing package metadata")?;

    let output_dir = std::path::absolute(&args.output_dir)
        .into_diagnostic()
        .context("resolving output directory")?;

    let subdir_dir = output_dir.join(recipe.subdir());
    std::fs::create_dir_all(&subdir_dir)
        .into_diagnostic()
        .context("creating output subdir")?;

    let filename = recipe.package_filename();
    let output_path = subdir_dir.join(&filename);

    pack_conda(&install_prefix, &output_path, &recipe)?;

    println!(
        "  {} Indexing channel at {}",
        console::style("→").blue(),
        output_dir.display()
    );
    index_fs(IndexFsConfig {
        channel: output_dir.clone(),
        target_platform: None, // discover all subdirs automatically
        repodata_patch: None,
        write_zst: true,
        write_shards: true,
        force: false, // incremental — only index new packages
        max_parallel: 4,
        multi_progress: None,
    })
    .await
    .map_err(|e| miette::miette!("{e:#}"))
    .context("indexing output channel")?;

    println!(
        "{} Built {}",
        console::style("✔").green(),
        console::style(&filename).cyan()
    );
    println!("  package → {}", output_path.display());
    println!("  channel → {}", output_dir.display());

    Ok(())
}

const BUILD_PRELUDE: &str = include_str!("../build_prelude.lua");

async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    recipe: &Recipe,
) -> miette::Result<()> {
    let wrapper_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating wrapper temp dir")?;

    let prelude_path = wrapper_dir.path().join("prelude.lua");
    std::fs::write(&prelude_path, BUILD_PRELUDE)
        .into_diagnostic()
        .context("writing build prelude")?;

    // The wrapper dofile()s the prelude then the user script.
    let wrapper_src = format!(
        "dofile({prelude:?})\ndofile({script:?})\n",
        prelude = prelude_path.to_string_lossy(),
        script = script.to_string_lossy(),
    );
    let wrapper_path = wrapper_dir.path().join("wrapper.lua");
    std::fs::write(&wrapper_path, &wrapper_src)
        .into_diagnostic()
        .context("writing build wrapper")?;

    // Prepend build_prefix/bin to PATH so the script can call any installed
    // build tools (luarocks, make, etc.).
    let build_bin = build_prefix.join("bin");
    let original_path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{original_path}", build_bin.display());

    let status = tokio::process::Command::new(lua_bin)
        .arg(&wrapper_path)
        .env("PREFIX", install_prefix)
        .env("SRC_DIR", src_dir)
        .env("BUILD_PREFIX", build_prefix)
        .env("PKG_NAME", &recipe.package.name)
        .env("PKG_VERSION", &recipe.package.version)
        .env("PKG_BUILD_NUM", recipe.package.build_number.to_string())
        .env("PATH", &new_path)
        .status()
        .await
        .into_diagnostic()
        .context("launching Lua interpreter")?;

    if !status.success() {
        miette::bail!(
            "Build script exited with status {}",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn write_package_metadata(install_prefix: &Path, recipe: &Recipe) -> miette::Result<()> {
    let info_dir = install_prefix.join("info");
    std::fs::create_dir_all(&info_dir)
        .into_diagnostic()
        .context("creating info/ directory")?;

    let noarch = if recipe.build.noarch {
        NoArchType::generic()
    } else {
        NoArchType::default()
    };

    let subdir = if recipe.build.noarch {
        Some("noarch".to_string())
    } else {
        Some(rattler_conda_types::Platform::current().to_string())
    };

    let index = IndexJson {
        name: PackageName::from_str(&recipe.package.name)
            .into_diagnostic()
            .with_context(|| format!("invalid package name `{}`", recipe.package.name))?,
        version: VersionWithSource::from_str(&recipe.package.version)
            .into_diagnostic()
            .with_context(|| format!("invalid version `{}`", recipe.package.version))?,
        build: recipe.build_string(),
        build_number: recipe.package.build_number,
        subdir,
        arch: None,
        platform: None,
        noarch,
        depends: recipe.requirements.run.clone(),
        constrains: vec![],
        experimental_extra_depends: Default::default(),
        features: None,
        license: recipe.package.license.clone(),
        license_family: None,
        purls: None,
        python_site_packages_path: None,
        track_features: vec![],
        timestamp: Some(
            rattler_conda_types::utils::TimestampMs::from_datetime_millis(chrono::Utc::now()),
        ),
    };

    let index_path = install_prefix.join(IndexJson::package_path());
    let index_json = serde_json::to_string_pretty(&index)
        .into_diagnostic()
        .context("serializing index.json")?;
    std::fs::write(&index_path, index_json)
        .into_diagnostic()
        .context("writing info/index.json")?;

    let paths = collect_paths_json(install_prefix).context("building paths.json")?;

    let paths_path = install_prefix.join(PathsJson::package_path());
    let paths_json = serde_json::to_string_pretty(&paths)
        .into_diagnostic()
        .context("serializing paths.json")?;
    std::fs::write(&paths_path, paths_json)
        .into_diagnostic()
        .context("writing info/paths.json")?;

    Ok(())
}

fn collect_paths_json(prefix: &Path) -> miette::Result<PathsJson> {
    let mut entries = Vec::new();

    for entry in WalkDir::new(prefix).into_iter().filter_map(|e| e.ok()) {
        let meta = entry.metadata().into_diagnostic()?;
        if !meta.is_file() {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(prefix)
            .into_diagnostic()
            .context("stripping prefix from path")?
            .to_path_buf();

        // Compute SHA-256 and size for integrity checking.
        let (sha256, size) = sha256_and_size(abs_path)?;

        entries.push(PathsEntry {
            relative_path: rel_path,
            no_link: false,
            path_type: PathType::HardLink,
            prefix_placeholder: None,
            sha256: Some(sha256),
            size_in_bytes: Some(size),
        });
    }

    Ok(PathsJson {
        paths: entries,
        paths_version: 1,
    })
}

fn sha256_and_size(path: &Path) -> miette::Result<(rattler_digest::Sha256Hash, u64)> {
    use std::io::Read;
    let file = File::open(path)
        .into_diagnostic()
        .with_context(|| format!("opening `{}`", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = reader.read(&mut buf).into_diagnostic()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((hasher.finalize(), size))
}

fn pack_conda(install_prefix: &Path, output_path: &Path, recipe: &Recipe) -> miette::Result<()> {
    // Collect all files relative to the install prefix.
    let files: Vec<PathBuf> = WalkDir::new(install_prefix)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    if files.is_empty() {
        miette::bail!(
            "The build script did not install any files into PREFIX (`{}`). \
             Make sure your build.lua copies files to `os.getenv(\"PREFIX\")`.",
            install_prefix.display()
        );
    }

    println!(
        "  {} Packing {} files…",
        console::style("→").blue(),
        files.len()
    );

    let writer = BufWriter::new(
        File::create(output_path)
            .into_diagnostic()
            .with_context(|| format!("creating output file `{}`", output_path.display()))?,
    );

    let out_name = format!(
        "{}-{}-{}",
        recipe.package.name,
        recipe.package.version,
        recipe.build_string()
    );

    let now = chrono::Utc::now();
    write_conda_package(
        writer,
        install_prefix,
        &files,
        CompressionLevel::Default,
        None, // use all available CPU threads for zstd
        &out_name,
        Some(&now),
        None, // no progress bar (already shown by our spinner)
    )
    .into_diagnostic()
    .context("writing .conda archive")?;

    Ok(())
}

fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
    let bin = prefix.join("bin").join("lua");
    if bin.exists() {
        return Ok(bin);
    }
    // Try lua5.4, lua5.3, … as fallbacks
    for minor in (1u8..=4u8).rev() {
        let versioned = prefix.join("bin").join(format!("lua5.{minor}"));
        if versioned.exists() {
            return Ok(versioned);
        }
    }
    miette::bail!(
        "No Lua interpreter found in `{}`. \
         Add `lua` to `[requirements] build` in your recipe.",
        prefix.join("bin").display()
    )
}
// ~/~ end
