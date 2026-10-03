//! `hallow-build windows-dist`: the Windows counterpart of `deb`. From a
//! Windows build (`HALLOW_TARGET=windows cargo hb build`) it makes
//!   - `hallow-<version>-win64-setup.exe`, the per-user NSIS installer,
//!   - `hallow-<version>-win64.zip`, the same files without installer,
//!   - `hallow-<version>-win64.complete.mar`, the unsigned complete update
//!     package (`cargo hb mar sign` signs it), and
//!   - `hallow-<version>-win64.json`, what the update manifest needs to
//!     know about the build (version, build ID), and
//!   - `windows-icons/`, the icons the build embedded (for the tests).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::config::{APP_NAME, Context, Target};
use crate::util::run;

/// MAR channel of Hallow's Windows updates (`MAR_CHANNEL_ID` in the
/// mozconfig).
pub const MAR_CHANNEL: &str = "hallow-release";

pub struct WindowsDist {
    pub installer: PathBuf,
    pub zip: PathBuf,
    pub mar: PathBuf,
    pub info: PathBuf,
}

fn mach(source: &Path, args: &[&str]) -> Result<()> {
    run(Command::new(source.join("mach"))
        .args(args)
        .current_dir(source)
        .env("MOZCONFIG", source.join("mozconfig"))
        .env("MOZBUILD_SKIP_INTERACTIVE", "1")
        .env("MACH_NO_TERMINAL_FOOTER", "1"))
}

/// The single file in `dir` whose name starts with `prefix` and ends with
/// `suffix`.
fn find_one(dir: &Path, prefix: &str, suffix: &str) -> Result<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with(prefix) && name.ends_with(suffix) {
            found.push(path);
        }
    }
    match found.len() {
        1 => Ok(found.pop().unwrap()),
        0 => bail!("no {prefix}*{suffix} in {}", dir.display()),
        _ => bail!("several {prefix}*{suffix} in {}: {found:?}", dir.display()),
    }
}

/// `Key=value` from the `[App]` section of an application.ini.
pub fn ini_value(ini: &str, key: &str) -> Option<String> {
    let mut in_app = false;
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            in_app = line == "[App]";
        } else if in_app && let Some(v) = line.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
            return Some(v.trim().to_owned());
        }
    }
    None
}

pub fn run_dist(ctx: &Context, output_dir: Option<PathBuf>) -> Result<WindowsDist> {
    if ctx.target != Target::Windows {
        bail!("windows-dist needs HALLOW_TARGET=windows (and a Windows build)");
    }
    let source = ctx.source_dir();
    let dist = ctx.objdir().join("dist");
    let ini = fs::read_to_string(dist.join(APP_NAME).join("application.ini"))
        .context("reading application.ini of the packaged build; run `cargo hb build` first")?;
    let app_version = ini_value(&ini, "Version").context("application.ini has no Version")?;
    if app_version != ctx.config.firefox.version {
        bail!(
            "packaged app is version {app_version}, but hallow.toml pins {}",
            ctx.config.firefox.version
        );
    }
    let build_id = ini_value(&ini, "BuildID").context("application.ini has no BuildID")?;
    let zip = find_one(&dist, &format!("{APP_NAME}-"), ".win64.zip")?;

    // The NSIS installer, from the files `mach package` staged.
    mach(&source, &["build", "installer"])?;
    let installer = find_one(
        &dist.join("install/sea"),
        &format!("{APP_NAME}-"),
        ".installer.exe",
    )?;

    let out = output_dir.unwrap_or_else(|| ctx.root.join("dist"));
    fs::create_dir_all(&out)?;
    let base = format!("{APP_NAME}-{}-win64", ctx.version());
    let result = WindowsDist {
        installer: out.join(format!("{base}-setup.exe")),
        zip: out.join(format!("{base}.zip")),
        mar: out.join(format!("{base}.complete.mar")),
        info: out.join(format!("{base}.json")),
    };

    // The complete update package, made by Mozilla's make_full_update.sh
    // with the build's own (host) mar tool.
    let mar_tool = dist.join("host/bin/mar");
    let _ = fs::remove_file(&result.mar);
    mach(
        &source,
        &[
            "repackage",
            "mar",
            "--input",
            &zip.to_string_lossy(),
            "--mar",
            &mar_tool.to_string_lossy(),
            "--output",
            &result.mar.to_string_lossy(),
            "--arch",
            "x86_64",
            "--mar-channel-id",
            MAR_CHANNEL,
        ],
    )?;
    let mar = crate::mar::Mar::read(&result.mar)?;
    if mar.channel != MAR_CHANNEL || mar.version != app_version {
        bail!(
            "{} is for {} {}, expected {MAR_CHANNEL} {app_version}",
            result.mar.display(),
            mar.channel,
            mar.version
        );
    }
    if !mar
        .entries
        .iter()
        .any(|e| e.name == format!("{APP_NAME}.exe"))
    {
        bail!("{} has no {APP_NAME}.exe", result.mar.display());
    }

    // The file users download shows Hallow's icon and name
    // (windows::brand_installer_stubs).
    let res = crate::pe::read_resources(&fs::read(&installer)?)?;
    let strings = crate::pe::version_strings(&res)?;
    let product = strings
        .iter()
        .find(|(k, _)| k == "ProductName")
        .map(|(_, v)| v.as_str());
    let sizes = crate::pe::icon_group_sizes(&res)?;
    if product != Some("Hallow") || sizes != crate::icons::WINDOWS_SMALL_ICON_SIZES {
        bail!(
            "{} is not branded as Hallow ({product:?}, icon sizes {sizes:?})",
            installer.display()
        );
    }
    fs::copy(&installer, &result.installer)?;
    fs::copy(&zip, &result.zip)?;
    // The icons the build embedded, for the Windows tests to compare the
    // executables' icons with.
    let icons = out.join("windows-icons");
    fs::create_dir_all(&icons)?;
    let branding = source.join("browser/branding/hallow");
    for name in ["firefox.ico", "pbmode.ico", "document.ico", "firefox64.ico"] {
        fs::copy(branding.join(name), icons.join(name))?;
    }
    let info = serde_json::json!({
        "version": ctx.version(),
        "appVersion": app_version,
        "buildID": build_id,
        "marChannel": MAR_CHANNEL,
    });
    fs::write(&result.info, serde_json::to_string_pretty(&info)? + "\n")?;
    for path in [&result.installer, &result.zip, &result.mar, &result.info] {
        eprintln!("wrote {}", path.display());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_app_section() {
        let ini = "[App]\nVendor=Mozilla\nVersion=157.0\nBuildID=20261003120000\n\
                   [Gecko]\nVersion=999\n";
        assert_eq!(ini_value(ini, "Version").as_deref(), Some("157.0"));
        assert_eq!(ini_value(ini, "BuildID").as_deref(), Some("20261003120000"));
        assert_eq!(ini_value(ini, "Missing"), None);
        assert_eq!(ini_value("[Gecko]\nVersion=1\n", "Version"), None);
    }
}
