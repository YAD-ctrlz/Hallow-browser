//! `hallow-build lint-prefs`: make sure Hallow's default prefs still mean
//! something in the Firefox version being built. Firefox renames and removes
//! prefs regularly; a stale pref silently does nothing.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use aho_corasick::AhoCorasick;
use anyhow::{Context as _, Result, bail};

/// Prefs read through a prefix in the code, so only the suffix appears in
/// the source (for example ActivityStream's `PREFS_CONFIG` table).
const DYNAMIC_PREFIXES: &[&str] = &[
    "browser.newtabpage.activity-stream.feeds.",
    "browser.newtabpage.activity-stream.",
    "browser.urlbar.",
];

/// Directories that cannot define prefs used by the browser.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "obj-hallow",
    "test",
    "tests",
    "testing",
    "third_party",
];

const EXTENSIONS: &[&str] = &[
    "c", "cpp", "h", "idl", "js", "json", "jsx", "mjs", "py", "rs", "toml", "yaml",
];

/// Names of all prefs set by `pref(...)`-style calls in a prefs file.
pub fn pref_names(js: &str) -> Vec<String> {
    js.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = ["pref(", "defaultPref(", "lockPref(", "sticky_pref("]
                .iter()
                .find_map(|call| line.strip_prefix(call))?;
            let rest = rest.trim_start().strip_prefix('"')?;
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
}

fn needles(pref: &str) -> Vec<String> {
    let mut needles = vec![
        format!("\"{pref}\""),
        format!("'{pref}'"),
        format!("name: {pref}\n"),
    ];
    if let Some(suffix) = DYNAMIC_PREFIXES
        .iter()
        .find_map(|prefix| pref.strip_prefix(prefix))
    {
        needles.push(format!("\"{suffix}\""));
    }
    needles
}

pub fn lint(prefs_file: &Path, source: &Path, strict: bool) -> Result<()> {
    if !source.join("modules/libpref").is_dir() {
        bail!("{} is not a Firefox source tree", source.display());
    }
    let text = fs::read_to_string(prefs_file)
        .with_context(|| format!("reading {}", prefs_file.display()))?;
    let prefs = pref_names(&text);

    let mut patterns = Vec::new();
    let mut owner = Vec::new();
    for (index, pref) in prefs.iter().enumerate() {
        for needle in needles(pref) {
            patterns.push(needle);
            owner.push(index);
        }
    }
    let matcher = AhoCorasick::new(&patterns)?;

    let mut found = BTreeSet::new();
    let mut stack = vec![source.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                // Our own branding pref file would make every pref "exist".
                let ours = entry.path() == source.join("browser/branding/hallow");
                if !SKIP_DIRS.contains(&name.as_ref()) && !ours {
                    stack.push(entry.path());
                }
                continue;
            }
            let wanted = name
                .rsplit_once('.')
                .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext));
            if !kind.is_file() || !wanted {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            scanned += 1;
            for hit in matcher.find_overlapping_iter(&bytes) {
                found.insert(owner[hit.pattern().as_usize()]);
            }
        }
    }

    let missing: Vec<&String> = prefs
        .iter()
        .enumerate()
        .filter(|(index, _)| !found.contains(index))
        .map(|(_, pref)| pref)
        .collect();
    eprintln!(
        "checked {} prefs against {scanned} source files: {} unknown",
        prefs.len(),
        missing.len()
    );
    for pref in &missing {
        eprintln!("  unknown pref: {pref}");
    }
    if strict && !missing.is_empty() {
        bail!(
            "{} prefs in prefs/hallow.js no longer exist in Firefox",
            missing.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_pref_names() {
        let js = r#"
// comment pref("ignored", 1);
pref("browser.uidensity", 1);
  pref( "a.b", "x" ); // trailing
sticky_pref("c", true);
lockPref("d", false);
"#;
        assert_eq!(pref_names(js), ["browser.uidensity", "a.b", "c", "d"]);
    }

    #[test]
    fn dynamic_prefixes_use_the_longest_match() {
        let n = needles("browser.newtabpage.activity-stream.feeds.telemetry");
        assert!(n.contains(&"\"telemetry\"".to_string()));
        let n = needles("browser.urlbar.trimHttps");
        assert!(n.contains(&"\"trimHttps\"".to_string()));
    }

    #[test]
    fn repository_prefs_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = fs::read_to_string(root.join("prefs/hallow.js")).unwrap();
        let names = pref_names(&text);
        assert!(names.len() > 20);
        let unique: BTreeSet<_> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate prefs in prefs/hallow.js"
        );
        // Every non-comment line that calls pref() must have been understood.
        let calls = text
            .lines()
            .filter(|l| l.trim_start().starts_with("pref("))
            .count();
        assert_eq!(calls, names.len());
    }
}
