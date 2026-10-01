//! Drive Mozilla's `mach` build system.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

use crate::config::Context;
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
