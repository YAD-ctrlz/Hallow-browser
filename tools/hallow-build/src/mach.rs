//! Drive Mozilla's `mach` build system.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::config::{APP_NAME, Context};
use crate::fetch::extract_tar_xz;
use crate::util::run;

fn mach(source: &Path, args: &[&str]) -> Result<()> {
    run(Command::new(source.join("mach"))
        .args(args)
        .current_dir(source)
        .env("MOZCONFIG", source.join("mozconfig"))
        // Never stop to ask questions in CI or scripts.
        .env("MOZBUILD_SKIP_INTERACTIVE", "1")
        .env("MACH_NO_TERMINAL_FOOTER", "1"))
}

pub fn build(ctx: &Context, bootstrap: bool) -> Result<()> {
    let source = ctx.source_dir();
    if !source.join("mozconfig").exists() {
        bail!(
            "{} is not prepared; run `cargo hb prepare` first",
            source.display()
        );
    }
    if bootstrap {
        mach(
            &source,
            &[
                "--no-interactive",
                "bootstrap",
                "--application-choice",
                "browser",
            ],
        )?;
    }
    mach(&source, &["build"])?;
    mach(&source, &["package"])?;
    eprintln!("packaged into {}", ctx.objdir().join("dist").display());
    Ok(())
}

/// Where `--enable-bootstrap` keeps Mozilla's toolchains.
fn mozbuild_state_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("MOZBUILD_STATE_PATH") {
        return Ok(dir.into());
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(Path::new(&home).join(".mozbuild"))
}

/// PGO training: run Mozilla's profile corpus (build/pgo) with an
/// instrumented build and write `merged.profdata` and the omni.ja access log
/// `en-US.log` into `out`. Needs a display, e.g. `xvfb-run cargo hb profile`.
pub fn profile(ctx: &Context, instrumented: &Path, out: &Path) -> Result<()> {
    let source = ctx.source_dir();
    if !source.join("mozconfig").exists() {
        bail!(
            "{} is not prepared; run `cargo hb prepare` first",
            source.display()
        );
    }
    // Configure bootstraps the toolchains, including llvm-profdata.
    mach(&source, &["configure"])?;
    let llvm_profdata = mozbuild_state_dir()?.join("clang/bin/llvm-profdata");
    if !llvm_profdata.exists() {
        bail!("{} not found after configure", llvm_profdata.display());
    }

    // Unpack the instrumented build where the final build stages its package,
    // so the recorded omni.ja paths match (Mozilla's CI does the same).
    let dist = ctx.objdir().join("dist");
    let app = dist.join(APP_NAME);
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    fs::create_dir_all(&dist)?;
    extract_tar_xz(instrumented, &dist)?;
    let binary = app.join(APP_NAME);
    if !binary.exists() {
        bail!("{} has no {APP_NAME} binary", instrumented.display());
    }

    // profileserver.py writes the raw profiles and its results into the
    // current directory, which must be the source tree.
    run(Command::new(source.join("mach"))
        .arg("python")
        .arg(source.join("build/pgo/profileserver.py"))
        .arg("--binary")
        .arg(&binary)
        .current_dir(&source)
        .env("MOZCONFIG", source.join("mozconfig"))
        .env("MOZBUILD_SKIP_INTERACTIVE", "1")
        .env("JARLOG_FILE", "en-US.log")
        .env("LLVM_PROFDATA", &llvm_profdata))?;

    fs::create_dir_all(out)?;
    for name in ["merged.profdata", "en-US.log"] {
        let from = source.join(name);
        let size = fs::metadata(&from)
            .with_context(|| format!("profile run did not produce {name}"))?
            .len();
        if size == 0 {
            bail!("profile run produced an empty {name}");
        }
        fs::copy(&from, out.join(name))?;
        eprintln!("{name}: {}", crate::util::human_bytes(size));
    }
    for entry in fs::read_dir(&source)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "profraw") {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
