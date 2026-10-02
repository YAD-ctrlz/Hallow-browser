//! `hallow-build prepare`: turn a pristine Firefox tree into a Hallow tree.
//!
//! 1. Add Hallow's own source files (`gecko/`, new files only) and apply
//!    `patches/*.patch` (strictly, no fuzz) for changes to Firefox's files.
//! 2. Create `browser/branding/hallow` from Mozilla's unofficial branding,
//!    overlaid with `branding/overlay`, icons rendered from `branding/logo.png`,
//!    the clean-UI stylesheet and Hallow's default prefs.
//! 3. Set the displayed version to the Hallow version and install the
//!    mozconfig.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

use crate::config::Context;
use crate::icons;
use crate::util::{copy_dir, write_file};

const BRANDING_DIR: &str = "browser/branding/hallow";

/// Firefox mascot artwork in toolkit/themes/shared/illustrations.
const KIT_ILLUSTRATIONS: [&str; 5] = [
    "kit-concerned.svg",
    "kit-confetti.svg",
    "kit-happy.svg",
    "kit-holding-lock.svg",
    "kit-in-circle.svg",
];

pub fn prepare(ctx: &Context) -> Result<()> {
    let source = ctx.source_dir();
    if !source.join("mach").exists() {
        bail!(
            "no Firefox source at {}; run `cargo hb fetch` first",
            source.display()
        );
    }
    install_gecko_files(&ctx.root.join("gecko"), &source)?;
    apply_patches(&ctx.root.join("patches"), &source)?;
    install_branding(&ctx.root, &source)?;
    install_version(&ctx.version(), &source)?;
    install_mozconfig(&ctx.root, &source)?;
    eprintln!("prepared {}", source.display());
    Ok(())
}

/// Copy the files under `gecko/` to the same paths in the Firefox tree.
/// They are files Hallow adds, which patches then hook up (moz.build
/// entries etc.); keeping them as plain files keeps them easy to edit.
/// Upstream must not have a file of the same name: that would mean
/// Firefox gained a file we would silently replace.
pub fn install_gecko_files(dir: &Path, source: &Path) -> Result<()> {
    let marker = source.join(".hallow-gecko-files");
    let installed: Vec<String> = fs::read_to_string(&marker)
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut names = Vec::new();
    for file in files {
        let relative = file.strip_prefix(dir)?.to_string_lossy().into_owned();
        let dest = source.join(&relative);
        if dest.exists() && !installed.contains(&relative) {
            bail!("gecko/{relative} would replace a Firefox file of the same name");
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&file, &dest).with_context(|| format!("copying gecko/{relative}"))?;
        eprintln!("added {relative}");
        names.push(relative);
    }
    fs::write(marker, names.join("\n"))?;
    Ok(())
}

/// Show the Hallow version (`157.0-7`) wherever Firefox shows its display
/// version (About dialog, Settings, about:support), so the browser reports
/// the version the package was installed as. Gecko's own version
/// (version.txt, used for compatibility checks and the user agent) stays
/// the Firefox version.
fn install_version(version: &str, source: &Path) -> Result<()> {
    write_file(
        &source.join("browser/config/version_display.txt"),
        format!("{version}\n"),
    )
}

fn patch_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut patches: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    patches.retain(|p| p.extension().is_some_and(|ext| ext == "patch"));
    patches.sort();
    Ok(patches)
}

/// `git apply` in a directory that is not a git checkout behaves like a
/// strict `patch -p1`. The ceiling stops git from discovering a repository
/// above the Firefox tree (e.g. this one, when `work/` lives inside it),
/// which would make it silently skip every file.
fn git_apply(source: &Path, patch: &Path, extra: &[&str]) -> Result<std::process::Output> {
    let ceiling = source.parent().unwrap_or(source);
    Command::new("git")
        .arg("apply")
        .args(extra)
        .arg(patch)
        .current_dir(source)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output()
        .context("running git apply")
}

pub fn apply_patches(dir: &Path, source: &Path) -> Result<()> {
    for patch in patch_files(dir)? {
        let name = patch.file_name().unwrap().to_string_lossy();
        let check = git_apply(source, &patch, &["--check"])?;
        if !check.status.success() {
            if git_apply(source, &patch, &["--check", "--reverse"])?
                .status
                .success()
            {
                eprintln!("patch {name}: already applied");
                continue;
            }
            bail!(
                "patch {name} does not apply to this Firefox version; refresh it.\n{}",
                String::from_utf8_lossy(&check.stderr)
            );
        }
        let applied = git_apply(source, &patch, &["--verbose"])?;
        if !applied.status.success() {
            bail!(
                "patch {name} failed:\n{}",
                String::from_utf8_lossy(&applied.stderr)
            );
        }
        eprintln!("patch {name}: applied");
    }
    Ok(())
}

pub fn install_branding(root: &Path, source: &Path) -> Result<()> {
    let branding = root.join("branding");
    let dest = source.join(BRANDING_DIR);
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }

    // Start from Mozilla's unofficial branding so every file the build system
    // expects (including Windows/macOS assets) exists, then replace what we own.
    copy_dir(&source.join("browser/branding/unofficial"), &dest)
        .context("copying browser/branding/unofficial")?;
    copy_dir(&branding.join("overlay"), &dest).context("copying branding/overlay")?;

    // App icons (Gecko installs default<N>.png as the window icons) and
    // about-dialog artwork, rendered from the master logo.
    let logo = icons::load_logo(&branding)?;
    for size in icons::BRANDING_SIZES {
        write_file(
            &dest.join(format!("default{size}.png")),
            icons::render_icon(&logo, size)?,
        )?;
    }
    let content = dest.join("content");
    for (name, w, h) in [
        ("about-logo.png", 192, 192),
        ("about-logo@2x.png", 384, 384),
        ("about-logo-private.png", 192, 192),
        ("about-logo-private@2x.png", 384, 384),
        ("about.png", 300, 236),
    ] {
        write_file(&content.join(name), icons::render_png(&logo, w, h)?)?;
    }
    write_file(
        &content.join("about-logo.svg"),
        icons::logo_svg(&logo, 512)?,
    )?;
    for name in ["about-wordmark.svg", "firefox-wordmark.svg"] {
        fs::copy(branding.join("wordmark.svg"), content.join(name))?;
    }

    // Stylesheets ship in the branding package: hallow.css is linked from
    // browser.xhtml (patches/0001), hallow-home.css from the new tab page
    // (patches/0005).
    let jar = content.join("jar.mn");
    let mut manifest = fs::read_to_string(&jar)?;
    for sheet in ["hallow.css", "hallow-home.css"] {
        fs::copy(root.join("ui").join(sheet), content.join(sheet))?;
        let entry = format!("content/branding/{sheet}");
        if !manifest.contains(&entry) {
            if !manifest.ends_with('\n') {
                manifest.push('\n');
            }
            manifest.push_str(&format!("  {entry}\n"));
        }
    }
    // Firefox's "Kit" fox illustrations appear in Settings and notifications;
    // a chrome override shows the Hallow logo instead, without patching the
    // pages that use them.
    write_file(
        &content.join("hallow-illustration.svg"),
        icons::illustration_svg(&logo)?,
    )?;
    if !manifest.contains("hallow-illustration.svg") {
        manifest.push_str("  content/branding/hallow-illustration.svg\n");
        for kit in KIT_ILLUSTRATIONS {
            manifest.push_str(&format!(
                "% override chrome://global/skin/illustrations/{kit} \
                 chrome://branding/content/hallow-illustration.svg\n"
            ));
        }
    }
    fs::write(&jar, manifest)?;

    // Default prefs. The branding pref file is loaded after firefox.js, so
    // these override Firefox's defaults while staying user-changeable.
    let pref_file = dest.join("pref/firefox-branding.js");
    let mut prefs = fs::read_to_string(&pref_file)?;
    prefs.push_str("\n// ---- Hallow defaults (prefs/hallow.js) ----\n");
    prefs.push_str(&fs::read_to_string(root.join("prefs/hallow.js"))?);
    fs::write(&pref_file, prefs)?;

    eprintln!("installed branding into {}", dest.display());
    Ok(())
}

fn install_mozconfig(root: &Path, source: &Path) -> Result<()> {
    fs::copy(root.join("mozconfig"), source.join("mozconfig")).context("copying mozconfig")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fake Firefox tree with just what `install_branding` touches.
    fn fake_source(dir: &Path) {
        let unofficial = dir.join("browser/branding/unofficial");
        write_file(
            &unofficial.join("content/jar.mn"),
            "browser.jar:\n  content/branding/about.png\n",
        )
        .unwrap();
        write_file(
            &unofficial.join("pref/firefox-branding.js"),
            "pref(\"a\", 1);\n",
        )
        .unwrap();
        write_file(
            &unofficial.join("configure.sh"),
            "MOZ_APP_DISPLAYNAME=Nightly\n",
        )
        .unwrap();
        write_file(&unofficial.join("firefox.ico"), "ico").unwrap();
    }

    #[test]
    fn branding_overlay() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let tmp = std::env::temp_dir().join(format!("hallow-branding-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fake_source(&tmp);

        install_branding(&root, &tmp).unwrap();
        // Running twice must be idempotent.
        install_branding(&root, &tmp).unwrap();

        let dest = tmp.join(BRANDING_DIR);
        let configure = fs::read_to_string(dest.join("configure.sh")).unwrap();
        assert!(configure.contains("MOZ_APP_DISPLAYNAME=Hallow"));
        assert!(
            dest.join("firefox.ico").exists(),
            "unofficial files are kept"
        );
        assert!(dest.join("default256.png").exists());
        assert!(dest.join("content/about-logo@2x.png").exists());
        let jar = fs::read_to_string(dest.join("content/jar.mn")).unwrap();
        assert_eq!(jar.matches("content/branding/hallow.css").count(), 1);
        assert_eq!(jar.matches("content/branding/hallow-home.css").count(), 1);
        assert!(dest.join("content/hallow-home.css").exists());
        assert_eq!(
            jar.matches("% override chrome://global/skin/illustrations/kit-")
                .count(),
            5
        );
        assert!(dest.join("content/hallow-illustration.svg").exists());
        let svg = fs::read_to_string(dest.join("content/about-logo.svg")).unwrap();
        assert!(svg.contains("data:image/png;base64,"));
        let prefs = fs::read_to_string(dest.join("pref/firefox-branding.js")).unwrap();
        assert!(prefs.starts_with("pref(\"a\", 1);"));
        assert_eq!(prefs.matches("Hallow defaults").count(), 1);
        let ftl = fs::read_to_string(dest.join("locales/en-US/brand.ftl")).unwrap();
        assert!(ftl.contains("-brand-short-name = Hallow"));

        fs::remove_dir_all(&tmp).unwrap();
    }
}
