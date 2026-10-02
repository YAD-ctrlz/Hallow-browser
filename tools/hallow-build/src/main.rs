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
    /// Print the Hallow version (for example `157.0-1`, or `157.0-1~dev42`
    /// with HALLOW_VERSION_SUFFIX).
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
    /// PGO training run: profile an instrumented build (needs a display).
    Profile {
        /// `.tar.xz` from `mach package` of a HALLOW_PGO=generate build.
        instrumented: PathBuf,
        /// Where to write merged.profdata and en-US.log.
        #[arg(long)]
        out: PathBuf,
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
        /// Armored public key of the update channel. Defaults to the Hallow
        /// archive key, packaging/linux/hallow-archive-keyring.asc.
        #[arg(long)]
        channel_key: Option<PathBuf>,
        /// APT repository URI of the update channel. Defaults to Hallow's
        /// stable channel.
        #[arg(long, default_value = deb::STABLE_REPOSITORY)]
        channel_uri: String,
        /// Fail instead of building a package without an update channel.
        #[arg(long)]
        require_update_channel: bool,
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
                println!("{}", ctx.version());
            }
            Ok(())
        }
        Command::Fetch => fetch::run(&ctx),
        Command::Prepare => branding::prepare(&ctx),
        Command::Build { bootstrap } => mach::build(&ctx, bootstrap),
        Command::Profile { instrumented, out } => mach::profile(&ctx, &instrumented, &out),
        Command::Deb {
            input,
            output_dir,
            channel_key,
            channel_uri,
            require_update_channel,
        } => {
            let key = channel_key
                .unwrap_or_else(|| ctx.root.join("packaging/linux/hallow-archive-keyring.asc"));
            let channel = if key.exists() {
                Some(deb::UpdateChannel {
                    key,
                    uri: channel_uri,
                })
            } else if require_update_channel {
                anyhow::bail!(
                    "{} is missing; see docs/RELEASING.md to set up the signing key",
                    key.display()
                );
            } else {
                None
            };
            let path = deb::run(&ctx, input, output_dir, channel)?;
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
