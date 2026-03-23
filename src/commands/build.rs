// ~/~ begin <<book/src/ch10-build.md#src/commands/build.rs>>[init]
// ~/~ begin <<book/src/ch10-build.md#build-imports>>[init]
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
use crate::manifest::{Manifest, MANIFEST_FILENAME};
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-args>>[init]
#[derive(Debug, Parser)]
pub struct Args {
    /// Directory where the built `.conda` file is written.
    ///
    /// Defaults to `./output/`.
    #[clap(long, default_value = "output")]
    pub output_dir: PathBuf,
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-execute>>[init]
pub async fn execute(args: Args) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch10-build.md#parse-manifest>>[init]
    let cwd = env::current_dir().into_diagnostic()?;
    
    let (_manifest_path, manifest) = Manifest::find_in_dir(&cwd)?;
    
    let build_config = manifest.build.as_ref().ok_or_else(|| {
        miette::miette!(
            "No [build] section in `{MANIFEST_FILENAME}`. \
             Add one to make this project buildable, or run \
             `shot init --library` to start a new library project."
        )
    })?;
    
    let version = manifest.project.version.as_deref().ok_or_else(|| {
        miette::miette!(
            "No `version` in [project]. \
             A version is required to build a package."
        )
    })?;
    
    println!(
        "Building {} {} (build {})",
        console::style(&manifest.project.name).cyan(),
        version,
        manifest.build_string(),
    );
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#setup-dirs>>[init]
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
    
    let src_dir = std::path::absolute(&cwd)
        .into_diagnostic()
        .context("resolving SRC_DIR")?;
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#install-deps>>[init]
    if !manifest.dependencies.is_empty() {
        println!(
            "  {} Installing {} build dependencies…",
            console::style("→").blue(),
            manifest.dependencies.len()
        );
        install_from_manifest(&manifest, build_prefix.clone()).await?;
    }
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#run-script-call>>[init]
    let script_path = cwd.join(&build_config.script);
    if !script_path.exists() {
        miette::bail!(
            "Build script `{}` not found (expected at `{}`)",
            build_config.script,
            script_path.display()
        );
    }
    
    let lua_bin = find_lua(&build_prefix)?;
    
    println!(
        "  {} Running build script `{}`",
        console::style("→").blue(),
        build_config.script
    );
    
    run_build_script(
        &lua_bin,
        &script_path,
        &install_prefix,
        &src_dir,
        &build_prefix,
        &manifest,
    )
    .await?;
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#pack-and-index>>[init]
    write_package_metadata(&install_prefix, &manifest).context("writing package metadata")?;
    
    let output_dir = std::path::absolute(&args.output_dir)
        .into_diagnostic()
        .context("resolving output directory")?;
    
    let subdir_dir = output_dir.join(manifest.subdir());
    std::fs::create_dir_all(&subdir_dir)
        .into_diagnostic()
        .context("creating output subdir")?;
    
    let filename = manifest.package_filename()?;
    let output_path = subdir_dir.join(&filename);
    
    pack_conda(&install_prefix, &output_path, &manifest)?;
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#pack-and-index>>[1]
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
        force: false, // incremental (only index new packages)
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
    // ~/~ end
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-prelude-const>>[init]
const BUILD_PRELUDE: &str = include_str!("../build_prelude.lua");
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-run-script>>[init]
async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch10-build.md#create-wrapper>>[init]
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
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#exec-lua>>[init]
    // Prepend build_prefix bin directories to PATH so the script can call
    // any installed build tools (luarocks, make, etc.).
    // On Windows, conda puts binaries in Library/bin as well as bin.
    let original_path = env::var("PATH").unwrap_or_default();
    let path_sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = if cfg!(windows) {
        format!(
            "{}{path_sep}{}{path_sep}{original_path}",
            build_prefix.join("Library").join("bin").display(),
            build_prefix.join("bin").display(),
        )
    } else {
        format!(
            "{}{path_sep}{original_path}",
            build_prefix.join("bin").display(),
        )
    };
    
    let build_config = manifest
        .build
        .as_ref()
        .expect("[build] section validated in execute()");
    
    let status = tokio::process::Command::new(lua_bin)
        .arg(&wrapper_path)
        .env("PREFIX", install_prefix)
        .env("SRC_DIR", src_dir)
        .env("BUILD_PREFIX", build_prefix)
        .env("PKG_NAME", &manifest.project.name)
        .env(
            "PKG_VERSION",
            manifest.project.version.as_deref().unwrap_or("0.0.0"),
        )
        .env("PKG_BUILD_NUM", build_config.build_number.to_string())
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
    // ~/~ end
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-write-metadata>>[init]
fn write_package_metadata(install_prefix: &Path, manifest: &Manifest) -> miette::Result<()> {
    // ~/~ begin <<book/src/ch10-build.md#create-index-json>>[init]
    let info_dir = install_prefix.join("info");
    std::fs::create_dir_all(&info_dir)
        .into_diagnostic()
        .context("creating info/ directory")?;
    
    let build_config = manifest
        .build
        .as_ref()
        .expect("[build] section validated in execute()");
    
    let noarch = if build_config.noarch {
        NoArchType::generic()
    } else {
        NoArchType::default()
    };
    
    let subdir = if build_config.noarch {
        Some("noarch".to_string())
    } else {
        Some(rattler_conda_types::Platform::current().to_string())
    };
    
    let version_str = manifest.project.version.as_deref().unwrap_or("0.0.0");
    
    let index = IndexJson {
        name: PackageName::from_str(&manifest.project.name)
            .into_diagnostic()
            .with_context(|| format!("invalid package name `{}`", manifest.project.name))?,
        version: VersionWithSource::from_str(version_str)
            .into_diagnostic()
            .with_context(|| format!("invalid version `{}`", version_str))?,
        build: manifest.build_string(),
        build_number: build_config.build_number,
        subdir,
        arch: None,
        platform: None,
        noarch,
        depends: manifest
            .dependencies
            .iter()
            .map(|(name, spec)| {
                if spec == "*" {
                    name.clone()
                } else {
                    format!("{name} {spec}")
                }
            })
            .collect(),
        constrains: vec![],
        experimental_extra_depends: Default::default(),
        features: None,
        license: manifest.project.license.clone(),
        license_family: None,
        purls: None,
        python_site_packages_path: None,
        track_features: vec![],
        timestamp: Some(
            rattler_conda_types::utils::TimestampMs::from_datetime_millis(chrono::Utc::now()),
        ),
    };
    // ~/~ end
    // ~/~ begin <<book/src/ch10-build.md#write-meta-files>>[init]
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
    // ~/~ end
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-collect-paths>>[init]
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
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-sha256>>[init]
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
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-pack-conda>>[init]
fn pack_conda(
    install_prefix: &Path,
    output_path: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
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
        manifest.project.name,
        manifest.project.version.as_deref().unwrap_or("0.0.0"),
        manifest.build_string()
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
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-find-lua>>[init]
fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
    // On Windows, conda installs binaries in Library/bin/ with .exe extension;
    // on Unix they go in bin/ without an extension.
    let bin_dirs: &[&str] = if cfg!(windows) {
        &["Library/bin", "bin"]
    } else {
        &["bin"]
    };
    let exe_ext = if cfg!(windows) { ".exe" } else { "" };

    for bin_dir in bin_dirs {
        let lua = prefix.join(bin_dir).join(format!("lua{exe_ext}"));
        if lua.exists() {
            return Ok(lua);
        }
        // Try lua5.4, lua5.3, … as fallbacks
        for minor in (1u8..=4u8).rev() {
            let versioned = prefix.join(bin_dir).join(format!("lua5.{minor}{exe_ext}"));
            if versioned.exists() {
                return Ok(versioned);
            }
        }
    }

    let searched: Vec<_> = bin_dirs
        .iter()
        .map(|d| prefix.join(d).display().to_string())
        .collect();
    miette::bail!(
        "No Lua interpreter found in `{}`. \
         Add `lua` to [dependencies] in moonshot.toml.",
        searched.join("`, `")
    )
}
// ~/~ end
// ~/~ end
