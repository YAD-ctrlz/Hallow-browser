//! `hallow-build preview <firefox-dir>`: layer Hallow's default prefs and
//! chrome stylesheet onto an official Firefox build of the same version.
//! This gives a faithful look at the UI in minutes instead of a multi-hour
//! Gecko build. It is a development aid only; release builds compile the
//! same files in through the branding package and patches/.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use crate::config::Context;
use crate::util::write_file;

/// Loaded by Gecko from `<install>/defaults/pref`; enables autoconfig.
const AUTOCONFIG_JS: &str = r#"// Hallow UI preview only.
pref("general.config.filename", "hallow-preview.cfg");
pref("general.config.obscure_value", 0);
pref("general.config.sandbox_enabled", false);
"#;

/// Autoconfig script: link the stylesheet into every browser window, the
/// same way patches/0001 does in real builds.
const PREVIEW_CFG: &str = r#"// Hallow UI preview: first line of an autoconfig file must be a comment.
try {
  const css = Services.dirsvc.get("GreD", Ci.nsIFile);
  css.append("hallow-preview.css");
  const href = Services.io.newFileURI(css).spec;
  Services.obs.addObserver(win => {
    win.addEventListener("DOMContentLoaded", () => {
      const doc = win.document;
      if (doc.documentURI != "chrome://browser/content/browser.xhtml") {
        return;
      }
      const link = doc.createElementNS("http://www.w3.org/1999/xhtml", "link");
      link.rel = "stylesheet";
      link.href = href;
      doc.head.append(link);
    }, { once: true });
  }, "domwindowopened");
} catch (e) {
  Cu.reportError(e);
}
"#;

/// Official Firefox builds show first-run notices that Hallow (an unofficial
/// build) never does; hide them so the preview matches.
const PREVIEW_ONLY_PREFS: &str = r#"
// ---- UI preview only: first-run notices of official Firefox builds ----
pref("termsofuse.bypassNotification", true);
pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
"#;

pub fn apply(ctx: &Context, firefox_dir: &Path) -> Result<()> {
    if !firefox_dir.join("application.ini").exists() || !firefox_dir.join("browser").is_dir() {
        bail!("{} is not an unpacked Firefox build", firefox_dir.display());
    }
    // `$app/defaults/preferences` loads after omni.ja's firefox.js, so these
    // override Firefox's defaults exactly like the branding pref file does.
    let mut prefs = fs::read_to_string(ctx.root.join("prefs/hallow.js"))?;
    prefs.push_str(PREVIEW_ONLY_PREFS);
    write_file(
        &firefox_dir.join("browser/defaults/preferences/hallow.js"),
        prefs,
    )?;
    write_file(
        &firefox_dir.join("defaults/pref/autoconfig.js"),
        AUTOCONFIG_JS,
    )?;
    write_file(&firefox_dir.join("hallow-preview.cfg"), PREVIEW_CFG)?;
    fs::copy(
        ctx.root.join("ui/hallow.css"),
        firefox_dir.join("hallow-preview.css"),
    )?;
    eprintln!(
        "applied Hallow prefs and stylesheet to {}",
        firefox_dir.display()
    );
    Ok(())
}
