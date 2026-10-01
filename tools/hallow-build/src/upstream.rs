//! Track the newest Firefox release (`hallow-build bump`).

use std::fs;

use anyhow::{Context as _, Result, bail};

use crate::config::{CONFIG_FILE, Context, validate_version, version_key};
use crate::fetch::published_sha512;
use crate::net;
use crate::util::github_output;

const PRODUCT_DETAILS: &str = "https://product-details.mozilla.org/1.0/firefox_versions.json";

pub fn latest_release() -> Result<String> {
    let json: serde_json::Value = serde_json::from_str(&net::get_string(PRODUCT_DETAILS)?)?;
    let version = json["LATEST_FIREFOX_VERSION"]
        .as_str()
        .context("LATEST_FIREFOX_VERSION missing from product-details")?;
    Ok(version.to_string())
}

pub fn bump(ctx: &Context, requested: Option<String>) -> Result<()> {
    let current = &ctx.config.firefox.version;
    let target = match requested {
        Some(version) => version,
        None => latest_release()?,
    };
    validate_version(&target)?;

    let newer = version_key(&target) > version_key(current);
    let unpinned = target == *current && ctx.config.firefox.sha512.is_empty();
    if !newer && !unpinned {
        eprintln!("Firefox {current} is current (latest release: {target})");
        github_output(&[("changed", "false"), ("version", current)])?;
        return Ok(());
    }
    if version_key(&target) < version_key(current) {
        bail!("refusing to move from Firefox {current} back to {target}");
    }

    let sha512 = published_sha512(&target)?;
    let path = ctx.root.join(CONFIG_FILE);
    let mut text = fs::read_to_string(&path)?;
    text = set_value(&text, "firefox", "version", &format!("\"{target}\""))?;
    text = set_value(&text, "firefox", "sha512", &format!("\"{sha512}\""))?;
    if newer {
        text = set_value(&text, "hallow", "revision", "1")?;
    }
    fs::write(&path, text)?;

    let package_version = if newer {
        format!("{target}-1")
    } else {
        ctx.config.package_version()
    };
    eprintln!("hallow.toml now pins Firefox {target} ({package_version})");
    github_output(&[
        ("changed", "true"),
        ("version", &target),
        ("package_version", &package_version),
    ])?;
    Ok(())
}

/// Replace `key = ...` inside `[section]` while keeping comments and layout.
fn set_value(text: &str, section: &str, key: &str, value: &str) -> Result<String> {
    let header = format!("[{section}]");
    let mut in_section = false;
    let mut replaced = false;
    let mut out = String::with_capacity(text.len() + value.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == header;
        }
        let is_key = trimmed
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if in_section && !replaced && is_key {
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            out.push_str(&format!("{key} = {value}{newline}"));
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    if !replaced {
        bail!("no `{key}` in [{section}] of {CONFIG_FILE}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_the_target_key() {
        let text = "# top\n[firefox]\n# keep me\nversion = \"157.0\"\nsha512 = \"\"\n\n[hallow]\nrevision = 3\n";
        let out = set_value(text, "firefox", "version", "\"158.0\"").unwrap();
        let out = set_value(&out, "hallow", "revision", "1").unwrap();
        assert_eq!(
            out,
            "# top\n[firefox]\n# keep me\nversion = \"158.0\"\nsha512 = \"\"\n\n[hallow]\nrevision = 1\n"
        );
        assert!(set_value(text, "hallow", "version", "1").is_err());
    }
}
