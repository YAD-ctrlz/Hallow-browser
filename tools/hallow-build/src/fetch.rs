//! Download, verify and unpack the Firefox source release.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use lzma_rust2::XzReader;

use crate::config::Context;
use crate::net;
use crate::util::{human_bytes, sha512_file};

pub const ARCHIVE: &str = "https://archive.mozilla.org/pub/firefox/releases";

/// Marker written once a source tree has been fully unpacked.
const EXTRACTED_MARKER: &str = ".hallow-extracted";

pub fn source_url(version: &str) -> String {
    format!("{ARCHIVE}/{version}/source/firefox-{version}.source.tar.xz")
}

/// Look up the source tarball's SHA-512 in Mozilla's SHA512SUMS for `version`.
pub fn published_sha512(version: &str) -> Result<String> {
    let url = format!("{ARCHIVE}/{version}/SHA512SUMS");
    let sums = net::get_string(&url)?;
    find_in_sums(&sums, &format!("source/firefox-{version}.source.tar.xz"))
        .with_context(|| format!("no source tarball entry in {url}"))
}

fn find_in_sums(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim() == file && hash.len() == 128).then(|| hash.to_ascii_lowercase())
    })
}

pub fn run(ctx: &Context) -> Result<()> {
    let version = &ctx.config.firefox.version;
    fs::create_dir_all(&ctx.work)?;

    let expected = if ctx.config.firefox.sha512.is_empty() {
        let hash = published_sha512(version)?;
        eprintln!("hallow.toml does not pin a hash; using Mozilla's SHA512SUMS: {hash}");
        hash
    } else {
        ctx.config.firefox.sha512.to_ascii_lowercase()
    };

    let tarball = ctx.work.join(ctx.tarball_name());
    let have_valid_tarball = tarball.exists() && {
        eprintln!("verifying existing {}", tarball.display());
        sha512_file(&tarball)? == expected
    };
    if !have_valid_tarball {
        let url = source_url(version);
        eprintln!("downloading {url}");
        let actual = net::download(&url, &tarball)?;
        if actual != expected {
            let _ = fs::remove_file(&tarball);
            bail!("SHA-512 mismatch for {url}\n  expected {expected}\n  actual   {actual}");
        }
    }
    eprintln!(
        "verified firefox {version} source (sha512 {})",
        &expected[..16]
    );

    let source = ctx.source_dir();
    if source.join(EXTRACTED_MARKER).exists() {
        eprintln!("source already unpacked at {}", source.display());
        return Ok(());
    }
    if source.exists() {
        eprintln!("removing incomplete {}", source.display());
        fs::remove_dir_all(&source)?;
    }
    extract_tar_xz(&tarball, &ctx.work)?;
    if !source.join("mach").exists() {
        bail!(
            "{} did not unpack into {}",
            tarball.display(),
            source.display()
        );
    }
    fs::write(source.join(EXTRACTED_MARKER), version)?;
    eprintln!("unpacked into {}", source.display());
    Ok(())
}

/// Unpack a `.tar.xz` into `dest` using pure-Rust xz and tar readers.
pub fn extract_tar_xz(tarball: &Path, dest: &Path) -> Result<()> {
    eprintln!(
        "unpacking {} ({})",
        tarball.display(),
        human_bytes(fs::metadata(tarball)?.len())
    );
    let file = File::open(tarball).with_context(|| format!("opening {}", tarball.display()))?;
    let xz = XzReader::new(BufReader::with_capacity(1 << 20, file), true);
    let mut archive = tar::Archive::new(BufReader::with_capacity(1 << 20, xz));
    archive.set_preserve_mtime(true);
    archive.set_overwrite(true);
    archive
        .unpack(dest)
        .with_context(|| format!("unpacking {}", tarball.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha512sums() {
        let hash = "ab".repeat(64);
        let sums = format!(
            "{other}  linux-x86_64/en-US/firefox-157.0.tar.xz\n{hash}  source/firefox-157.0.source.tar.xz\n",
            other = "cd".repeat(64)
        );
        assert_eq!(
            find_in_sums(&sums, "source/firefox-157.0.source.tar.xz"),
            Some(hash)
        );
        assert_eq!(
            find_in_sums(&sums, "source/firefox-1.0.source.tar.xz"),
            None
        );
    }

    #[test]
    fn source_url_layout() {
        assert_eq!(
            source_url("157.0"),
            "https://archive.mozilla.org/pub/firefox/releases/157.0/source/firefox-157.0.source.tar.xz"
        );
    }
}
