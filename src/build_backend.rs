// ~/~ begin <<book/src/ch10-build.md#src/build_backend.rs>>[init]
// ~/~ begin <<book/src/ch10-build.md#build-backend-imports>>[init]
use std::path::{Path, PathBuf};

use miette::{Context, IntoDiagnostic};

use crate::manifest::Manifest;
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-context-struct>>[init]
/// Context passed to a [`BuildBackend`] when executing a build.
pub struct BuildContext<'a> {
    pub manifest: &'a Manifest,
    pub src_dir: PathBuf,
    pub install_prefix: PathBuf,
    pub build_prefix: PathBuf,
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#build-backend-trait>>[init]
/// A pluggable build backend.
///
/// Implement this trait to add support for new build-script languages
/// beyond Lua.
#[allow(dead_code)]
pub trait BuildBackend {
    /// Human-readable name of this backend, for log messages.
    fn name(&self) -> &str;

    /// Run the build script, installing files into `ctx.install_prefix`.
    fn run_build(
        &self,
        ctx: &BuildContext<'_>,
    ) -> impl std::future::Future<Output = miette::Result<()>> + Send;
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#lua-backend-const>>[init]
const BUILD_PRELUDE: &str = include_str!("build_prelude.lua");
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#lua-backend-struct>>[init]
/// The default build backend — runs a Lua build script.
pub struct LuaBuildBackend;
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#lua-backend-impl>>[init]
impl BuildBackend for LuaBuildBackend {
    fn name(&self) -> &str {
        "lua"
    }

    async fn run_build(&self, ctx: &BuildContext<'_>) -> miette::Result<()> {
        let build_config = ctx
            .manifest
            .build
            .as_ref()
            .expect("[build] section validated in execute()");

        let script_path = ctx.src_dir.join(&build_config.script);
        if !script_path.exists() {
            miette::bail!(
                "Build script `{}` not found (expected at `{}`)",
                build_config.script,
                script_path.display()
            );
        }

        let lua_bin = find_lua(&ctx.build_prefix)?;

        println!(
            "  {} Running build script `{}`",
            console::style("→").blue(),
            build_config.script
        );

        run_build_script(
            &lua_bin,
            &script_path,
            &ctx.install_prefix,
            &ctx.src_dir,
            &ctx.build_prefix,
            ctx.manifest,
        )
        .await
    }
}
// ~/~ end

// ~/~ begin <<book/src/ch10-build.md#lua-find-lua>>[init]
fn find_lua(prefix: &Path) -> miette::Result<PathBuf> {
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

// ~/~ begin <<book/src/ch10-build.md#lua-run-build-script>>[init]
async fn run_build_script(
    lua_bin: &Path,
    script: &Path,
    install_prefix: &Path,
    src_dir: &Path,
    build_prefix: &Path,
    manifest: &Manifest,
) -> miette::Result<()> {
    let wrapper_dir = tempfile::tempdir()
        .into_diagnostic()
        .context("creating wrapper temp dir")?;

    let prelude_path = wrapper_dir.path().join("prelude.lua");
    std::fs::write(&prelude_path, BUILD_PRELUDE)
        .into_diagnostic()
        .context("writing build prelude")?;

    let wrapper_src = format!(
        "dofile({prelude:?})\ndofile({script:?})\n",
        prelude = prelude_path.to_string_lossy(),
        script = script.to_string_lossy(),
    );
    let wrapper_path = wrapper_dir.path().join("wrapper.lua");
    std::fs::write(&wrapper_path, &wrapper_src)
        .into_diagnostic()
        .context("writing build wrapper")?;

    let original_path = std::env::var("PATH").unwrap_or_default();
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
}
// ~/~ end
// ~/~ end
