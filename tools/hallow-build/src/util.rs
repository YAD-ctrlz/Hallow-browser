use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha512};

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn sha512_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Recursively copy `src` into `dst`, keeping symlinks and permission bits.
pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            let _ = fs::remove_file(&to);
            symlink(fs::read_link(&from)?, &to)?;
        } else if kind.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

pub fn write_file(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Run a command, streaming its output, and fail if it exits non-zero.
pub fn run(command: &mut Command) -> Result<()> {
    eprintln!("+ {command:?}");
    let status = command
        .status()
        .with_context(|| format!("starting {:?}", command.get_program()))?;
    if !status.success() {
        bail!("{:?} failed with {status}", command.get_program());
    }
    Ok(())
}

/// Append `key=value` lines to `$GITHUB_OUTPUT` when running in Actions.
pub fn github_output(pairs: &[(&str, &str)]) -> io::Result<()> {
    let Ok(path) = std::env::var("GITHUB_OUTPUT") else {
        return Ok(());
    };
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    for (key, value) in pairs {
        writeln!(file, "{key}={value}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(human_bytes(512), "512.0 B");
        assert_eq!(human_bytes(3 << 20), "3.0 MiB");
    }
}
