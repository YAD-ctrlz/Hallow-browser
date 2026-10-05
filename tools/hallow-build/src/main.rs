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
mod mar;
mod net;
mod pe;
mod prefs;
mod preview;
mod upstream;
mod util;
mod windows;
mod winpkg;

use std::path::PathBuf;

use anyhow::{Context as _, Result};
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
    /// Merge raw profiles of a training run made elsewhere (Windows) for a
    /// HALLOW_PGO=use build.
    PgoMerge {
        /// Directory with the .profraw files.
        raw: PathBuf,
        /// Where to write merged.profdata.
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
    /// Make the Windows installer, zip and unsigned complete update (MAR)
    /// from a Windows build.
    WindowsDist {
        /// Where to write them. Defaults to `<root>/dist`.
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    /// Sign, verify or inspect MAR update packages (Windows updates).
    Mar {
        #[command(subcommand)]
        action: MarAction,
    },
    /// Write the update manifest (update-win64.xml) offering a signed MAR.
    UpdateXml {
        /// The signed complete MAR the manifest offers.
        #[arg(long)]
        mar: PathBuf,
        /// HTTPS URL the MAR is downloaded from.
        #[arg(long)]
        url: String,
        /// `hallow-<version>-win64.json` from `windows-dist`.
        #[arg(long)]
        info: PathBuf,
        /// Release notes URL.
        #[arg(long)]
        details_url: String,
        /// Build ID to announce instead of the build's own (update tests).
        #[arg(long)]
        build_id: Option<String>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Render the Hallow icon sets (Linux and Windows) into a directory.
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

#[derive(Subcommand)]
enum MarAction {
    /// Sign a MAR (replacing any signature) with a PEM private key and check
    /// the result against the key's DER certificate.
    Sign {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        cert: PathBuf,
        /// Rewrite the MAR channel ID (update tests).
        #[arg(long)]
        channel: Option<String>,
        /// Rewrite the product version (update tests).
        #[arg(long)]
        product_version: Option<String>,
    },
    /// Check that a MAR has exactly one valid signature by the certificate's
    /// key, as the updater does. Exits non-zero otherwise.
    Verify {
        file: PathBuf,
        #[arg(long)]
        cert: PathBuf,
    },
    /// Print a MAR's channel, version, signatures and files.
    Info { file: PathBuf },
}

fn mar_command(action: MarAction) -> Result<()> {
    match action {
        MarAction::Sign {
            input,
            output,
            key,
            cert,
            channel,
            product_version,
        } => {
            let mut package = mar::Mar::read(&input)?;
            if let Some(channel) = channel {
                package.channel = channel;
            }
            if let Some(version) = product_version {
                package.version = version;
            }
            let signed = mar::sign(&package, &key, &cert)?;
            std::fs::write(&output, signed)?;
            eprintln!(
                "signed {} ({} {})",
                output.display(),
                package.channel,
                package.version
            );
            Ok(())
        }
        MarAction::Verify { file, cert } => {
            if mar::verify(&file, &cert)? {
                eprintln!("{}: valid signature by {}", file.display(), cert.display());
                Ok(())
            } else {
                anyhow::bail!(
                    "{}: no valid signature by {}",
                    file.display(),
                    cert.display()
                )
            }
        }
        MarAction::Info { file } => {
            let package = mar::Mar::read(&file)?;
            println!("channel: {}", package.channel);
            println!("version: {}", package.version);
            for sig in &package.signatures {
                println!(
                    "signature: algorithm {}, {} bytes",
                    sig.algorithm,
                    sig.bytes.len()
                );
            }
            for entry in &package.entries {
                println!("{:o} {:>10} {}", entry.mode, entry.data.len(), entry.name);
            }
            Ok(())
        }
    }
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
        Command::PgoMerge { raw, out } => mach::pgo_merge(&ctx, &raw, &out),
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
        Command::WindowsDist { output_dir } => {
            winpkg::run_dist(&ctx, output_dir)?;
            Ok(())
        }
        Command::Mar { action } => mar_command(action),
        Command::UpdateXml {
            mar,
            url,
            info,
            details_url,
            build_id,
            output,
        } => {
            if !url.starts_with("https://") && !url.starts_with("http://127.0.0.1") {
                anyhow::bail!("update packages are served over HTTPS, not {url}");
            }
            let info: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&info)?)?;
            let field = |k: &str| {
                info[k]
                    .as_str()
                    .map(str::to_owned)
                    .with_context(|| format!("build info has no {k}"))
            };
            let data = std::fs::read(&mar)?;
            let package = mar::Mar::parse(&data)?;
            if package.signatures.len() != 1 {
                anyhow::bail!("{} is not signed", mar.display());
            }
            let sha512 = {
                use sha2::Digest;
                util::hex(&sha2::Sha512::digest(&data))
            };
            let build_id = match build_id {
                Some(id) => id,
                None => field("buildID")?,
            };
            let (version, app_version) = (field("version")?, field("appVersion")?);
            let offer = mar::UpdateOffer {
                display_version: &version,
                app_version: &app_version,
                build_id: &build_id,
                details_url: &details_url,
                mar_url: &url,
                mar_size: data.len() as u64,
                mar_sha512: &sha512,
            };
            std::fs::write(&output, mar::update_xml(Some(&offer)))?;
            eprintln!("wrote {}", output.display());
            Ok(())
        }
        Command::Icons { out } => {
            icons::render_all(&ctx.root.join("branding"), &out)?;
            windows::render_all(&icons::load_logo(&ctx.root.join("branding"))?, &out)
        }
        Command::LintPrefs { source, strict } => {
            let source = source.unwrap_or_else(|| ctx.source_dir());
            prefs::lint(&ctx.root.join("prefs/hallow.js"), &source, strict)
        }
        Command::Bump { version } => upstream::bump(&ctx, version),
        Command::Preview { firefox_dir } => preview::apply(&ctx, &firefox_dir),
    }
}
