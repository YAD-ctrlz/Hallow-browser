//! `hallow.toml` and the paths derived from it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

pub const CONFIG_FILE: &str = "hallow.toml";

/// Name of the browser binary, package and install directory.
pub const APP_NAME: &str = "hallow";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub firefox: Firefox,
    pub hallow: Hallow,
    pub package: Package,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Firefox {
    /// Upstream release, e.g. `157.0` or `157.0.1`.
    pub version: String,
    /// SHA-512 of `firefox-<version>.source.tar.xz`. Empty means "trust
    /// Mozilla's SHA512SUMS for this release".
    #[serde(default)]
    pub sha512: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hallow {
    /// Hallow build number for the same Firefox version, starting at 1.
    pub revision: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub maintainer: String,
    pub homepage: String,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(CONFIG_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let config: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        validate_version(&self.firefox.version)?;
        let hash = &self.firefox.sha512;
        if !hash.is_empty() && (hash.len() != 128 || !hash.bytes().all(|b| b.is_ascii_hexdigit())) {
            bail!("firefox.sha512 must be 128 hex characters or empty");
        }
        if self.hallow.revision == 0 {
            bail!("hallow.revision starts at 1");
        }
        Ok(())
    }

    /// Debian-style version: upstream version, then the Hallow revision.
    pub fn package_version(&self) -> String {
        format!("{}-{}", self.firefox.version, self.hallow.revision)
    }
}

/// Accept plain Firefox release versions: `157.0`, `157.0.1`.
pub fn validate_version(version: &str) -> Result<()> {
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !(2..=3).contains(&parts.len()) || !numeric {
        bail!("`{version}` is not a Firefox release version like 157.0 or 157.0.1");
    }
    Ok(())
}

/// Numeric key for comparing release versions (`157.0.1` > `157.0`).
pub fn version_key(version: &str) -> Vec<u64> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub struct Context {
    pub root: PathBuf,
    pub work: PathBuf,
    pub config: Config,
}

impl Context {
    pub fn new(root: Option<PathBuf>, work: Option<PathBuf>) -> Result<Self> {
        let root = match root {
            Some(root) => root,
            None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        };
        let root = root
            .canonicalize()
            .with_context(|| format!("repository root {} not found", root.display()))?;
        let config = Config::load(&root)?;
        let work = work.unwrap_or_else(|| root.join("work"));
        Ok(Self { root, work, config })
    }

    pub fn tarball_name(&self) -> String {
        format!("firefox-{}.source.tar.xz", self.config.firefox.version)
    }

    /// The unpacked Firefox tree (`<work>/firefox-<version>`).
    pub fn source_dir(&self) -> PathBuf {
        self.work
            .join(format!("firefox-{}", self.config.firefox.version))
    }

    /// Object directory; must match `MOZ_OBJDIR` in the mozconfig.
    pub fn objdir(&self) -> PathBuf {
        self.source_dir().join("obj-hallow")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(validate_version("157.0").is_ok());
        assert!(validate_version("157.0.1").is_ok());
        assert!(validate_version("157").is_err());
        assert!(validate_version("157.0b1").is_err());
        assert!(validate_version("140.0esr").is_err());
        assert!(version_key("157.0.1") > version_key("157.0"));
        assert!(version_key("158.0") > version_key("157.0.2"));
        assert!(version_key("100.0") > version_key("99.0.1"));
    }

    #[test]
    fn repository_config_is_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let config = Config::load(&root).unwrap();
        assert!(
            config
                .package_version()
                .starts_with(&config.firefox.version)
        );
    }
}
