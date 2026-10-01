//! `hallow-build` turns an upstream Firefox source release into Hallow.
//!
//! The pipeline is: `fetch` (download + verify + unpack Firefox) →
//! `prepare` (patches, branding, prefs, mozconfig) → `build` (mach) →
//! `deb` (Debian package). Everything Hallow adds on top of Gecko lives in
//! this repository; the Firefox tree itself is never committed.

mod branding;
mod config;
mod deb;
mod elf;
mod fetch;
mod icons;
mod mach;
mod net;
mod prefs;
mod preview;
mod upstream;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Context;

#[derive(Parser)]
#[command(version, about, propagate_version = true)]
struct Cli {
    /// Repository root. Defaults to the checkout this tool was built from.
    #[arg(long, global = true, env = "HALLOW_ROOT")]
    root: Option<PathBuf>,

    /// Directory for downloads, the Firefox source tree and build output.
    #[arg(long, global = true, env = "HALLOW_WORK_DIR")]
    work_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the Hallow package version (for example `157.0-1`).
    Version {
        /// Print only the upstream Firefox version.
        #[arg(long)]
        firefox: bool,
    },
    /// Download, verify and unpack the pinned Firefox source release.
    Fetch,
    /// Apply patches, branding, default prefs and the mozconfig to the source tree.
    Prepare,
    /// Build and package the browser with mach.
    Build {
        /// Run `mach bootstrap` first to install toolchains and system packages.
        #[arg(long)]
        bootstrap: bool,
    },
    /// Turn the packaged browser into a Debian package.
    Deb {
        /// Staged app directory or `.tar.xz` from `mach package`.
        /// Defaults to `<objdir>/dist/hallow`.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Where to write the `.deb`. Defaults to `<root>/dist`.
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Render the Hallow icon set into a directory.
    Icons { out: PathBuf },
    /// Check that every pref in prefs/hallow.js still exists in the Firefox source.
    LintPrefs {
        /// Firefox source tree. Defaults to the fetched tree.
        #[arg(long)]
        source: Option<PathBuf>,
        /// Exit with an error when a pref is missing instead of warning.
        #[arg(long)]
        strict: bool,
    },
    /// Point hallow.toml at the newest Firefox release.
    Bump {
        /// Use this Firefox version instead of asking Mozilla for the latest.
        #[arg(long)]
        version: Option<String>,
    },
    /// Apply Hallow's prefs and stylesheet to an official Firefox build so
    /// the UI can be previewed without compiling Gecko.
    Preview { firefox_dir: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = Context::new(cli.root, cli.work_dir)?;

    match cli.command {
        Command::Version { firefox } => {
            if firefox {
                println!("{}", ctx.config.firefox.version);
            } else {
                println!("{}", ctx.config.package_version());
            }
            Ok(())
        }
        Command::Fetch => fetch::run(&ctx),
        Command::Prepare => branding::prepare(&ctx),
        Command::Build { bootstrap } => mach::build(&ctx, bootstrap),
        Command::Deb { input, output_dir } => {
            let path = deb::run(&ctx, input, output_dir)?;
            println!("{}", path.display());
            Ok(())
        }
        Command::Icons { out } => icons::render_all(&ctx.root.join("branding"), &out),
        Command::LintPrefs { source, strict } => {
            let source = source.unwrap_or_else(|| ctx.source_dir());
            prefs::lint(&ctx.root.join("prefs/hallow.js"), &source, strict)
        }
        Command::Bump { version } => upstream::bump(&ctx, version),
        Command::Preview { firefox_dir } => preview::apply(&ctx, &firefox_dir),
    }
}
