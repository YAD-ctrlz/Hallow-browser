//! Just enough ELF64 parsing to list a binary's `DT_NEEDED` and `DT_SONAME`
//! entries, which drive the `.deb` dependency list.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

use anyhow::{Context, Result, bail};

const SHT_DYNAMIC: u32 = 6;
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_SONAME: u64 = 14;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Dynamic {
    pub soname: Option<String>,
    pub needed: Vec<String>,
}

pub fn is_elf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    File::open(path)
        .and_then(|f| f.read_exact_at(&mut magic, 0))
        .is_ok()
        && magic == *b"\x7fELF"
}

fn read<const N: usize>(file: &File, offset: u64) -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    file.read_exact_at(&mut buf, offset)?;
    Ok(buf)
}

fn u16_at(file: &File, offset: u64) -> Result<u16> {
    Ok(u16::from_le_bytes(read(file, offset)?))
}
fn u32_at(file: &File, offset: u64) -> Result<u32> {
    Ok(u32::from_le_bytes(read(file, offset)?))
}
fn u64_at(file: &File, offset: u64) -> Result<u64> {
    Ok(u64::from_le_bytes(read(file, offset)?))
}

/// Read the dynamic section of a little-endian ELF64 file.
pub fn dynamic(path: &Path) -> Result<Dynamic> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let ident: [u8; 6] = read(&file, 0)?;
    if &ident[..4] != b"\x7fELF" {
        bail!("{} is not an ELF file", path.display());
    }
    if ident[4] != 2 || ident[5] != 1 {
        bail!("{} is not a little-endian ELF64 file", path.display());
    }

    let shoff = u64_at(&file, 0x28)?;
    let shentsize = u64::from(u16_at(&file, 0x3a)?);
    let shnum = u64::from(u16_at(&file, 0x3c)?);
    let section = |index: u64| shoff + index * shentsize;

    let mut result = Dynamic::default();
    for index in 0..shnum {
        let header = section(index);
        if u32_at(&file, header + 4)? != SHT_DYNAMIC {
            continue;
        }
        let offset = u64_at(&file, header + 24)?;
        let size = u64_at(&file, header + 32)?;
        let strtab = section(u64::from(u32_at(&file, header + 40)?));
        let str_offset = u64_at(&file, strtab + 24)?;
        let str_size = u64_at(&file, strtab + 32)?;
        let mut strings = vec![0u8; str_size as usize];
        file.read_exact_at(&mut strings, str_offset)?;
        let string_at = |at: u64| -> Result<String> {
            let tail = strings
                .get(at as usize..)
                .context("dynamic string offset out of range")?;
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
        };

        for entry in 0..size / 16 {
            let tag = u64_at(&file, offset + entry * 16)?;
            let value = u64_at(&file, offset + entry * 16 + 8)?;
            match tag {
                DT_NULL => break,
                DT_NEEDED => result.needed.push(string_at(value)?),
                DT_SONAME => result.soname = Some(string_at(value)?),
                _ => {}
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_system_binaries() {
        // Any dynamically linked executable on a glibc system needs libc.
        let ls = Path::new("/bin/ls");
        if !ls.exists() {
            return;
        }
        assert!(is_elf(ls));
        let ls_dynamic = dynamic(ls).unwrap();
        assert!(
            ls_dynamic.needed.iter().any(|n| n == "libc.so.6"),
            "{ls_dynamic:?}"
        );

        let libc = [
            "/lib/x86_64-linux-gnu/libc.so.6",
            "/usr/lib/x86_64-linux-gnu/libc.so.6",
        ]
        .into_iter()
        .map(Path::new)
        .find(|p| p.exists());
        if let Some(libc) = libc {
            assert_eq!(dynamic(libc).unwrap().soname.as_deref(), Some("libc.so.6"));
        }
        assert!(!is_elf(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("Cargo.toml")
                .as_path()
        ));
    }
}
