//! Mozilla ARchive (MAR) update packages: reading, signing and verifying.
//!
//! Gecko's updater on Windows installs an update only from a MAR that
//!   - carries exactly one RSA-PKCS#1 v1.5 SHA-384 signature made by a key
//!     whose certificate is built into the updater (see
//!     `windows::install_update_certificates`),
//!   - names an accepted MAR channel (`hallow-release`) in its product
//!     information block, and
//!   - is for the installed application version or a later one.
//!
//! `mach repackage mar` (Mozilla's `make_full_update.sh`) makes the unsigned
//! package; this module signs it the way Mozilla's `signmar` does. The
//! private key stays with OpenSSL: it is only ever passed as a file to
//! `openssl dgst -sign`.
//!
//! Layout (all integers big-endian):
//! ```text
//! "MAR1" | offset to index: u32 | file size: u64
//! signature count: u32 | per signature: algorithm: u32, length: u32, bytes
//! additional block count: u32 | per block: size: u32, id: u32, data
//! file contents ...
//! index size: u32 | per file: offset: u32, length: u32, mode: u32, name\0
//! ```
//! A signature covers the whole file except the signature bytes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

const MAGIC: &[u8; 4] = b"MAR1";
/// RSA-PKCS#1 v1.5 with SHA-384, the only algorithm the updater accepts.
const SIGNATURE_ALGORITHM: u32 = 2;
const PRODUCT_INFO_BLOCK_ID: u32 = 1;
/// The product information block reserves room for the longest channel ID
/// (63 bytes) and version (31 bytes), as `mar_create.c` does.
const MAX_CHANNEL_LEN: usize = 63;
const MAX_VERSION_LEN: usize = 31;
const PRODUCT_INFO_BLOCK_SIZE: u32 = 4 + 4 + (MAX_CHANNEL_LEN + MAX_VERSION_LEN + 2) as u32;
/// Limits from libmar (`mar_private.h`, `mar.h`).
const MAX_SIGNATURES: u32 = 8;
const MAX_SIGNATURE_LENGTH: u32 = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// Unix permission bits.
    pub mode: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub algorithm: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mar {
    /// MAR channel ID from the product information block.
    pub channel: String,
    /// Application version the package installs.
    pub version: String,
    pub entries: Vec<Entry>,
    pub signatures: Vec<Signature>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.context("MAR is truncated")?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
}

fn c_string(bytes: &[u8]) -> Result<(String, usize)> {
    let len = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated string in MAR")?;
    let s = std::str::from_utf8(&bytes[..len]).context("MAR string is not UTF-8")?;
    Ok((s.to_owned(), len + 1))
}

impl Mar {
    pub fn parse(data: &[u8]) -> Result<Mar> {
        let mut r = Reader { data, pos: 0 };
        ensure!(r.take(4)? == MAGIC, "not a MAR file");
        let index_offset = r.u32()? as usize;
        let file_size = r.u64()?;
        ensure!(
            file_size == data.len() as u64,
            "MAR says it is {file_size} bytes but is {}",
            data.len()
        );
        let count = r.u32()?;
        ensure!(count <= MAX_SIGNATURES, "too many signatures ({count})");
        let mut signatures = Vec::new();
        for _ in 0..count {
            let algorithm = r.u32()?;
            let len = r.u32()?;
            ensure!(len <= MAX_SIGNATURE_LENGTH, "signature is too long");
            signatures.push(Signature {
                algorithm,
                bytes: r.take(len as usize)?.to_vec(),
            });
        }
        let blocks = r.u32()?;
        let (mut channel, mut version) = (None, None);
        for _ in 0..blocks {
            let size = r.u32()? as usize;
            ensure!(size >= 8, "invalid additional block");
            let id = r.u32()?;
            let body = r.take(size - 8)?;
            if id == PRODUCT_INFO_BLOCK_ID {
                let (c, used) = c_string(body)?;
                let (v, _) = c_string(&body[used..])?;
                channel = Some(c);
                version = Some(v);
            }
        }
        let (Some(channel), Some(version)) = (channel, version) else {
            bail!("MAR has no product information block");
        };

        let mut index = Reader {
            data,
            pos: index_offset,
        };
        let index_size = index.u32()? as usize;
        let index_end = index.pos + index_size;
        ensure!(index_end == data.len(), "MAR index does not end the file");
        let mut entries = Vec::new();
        while index.pos < index_end {
            let offset = index.u32()? as usize;
            let len = index.u32()? as usize;
            let mode = index.u32()?;
            let (name, used) = c_string(&data[index.pos..index_end])?;
            index.pos += used;
            let end = offset.checked_add(len).filter(|&e| e <= index_offset);
            let end = end.with_context(|| format!("{name} lies outside the MAR"))?;
            entries.push(Entry {
                name,
                mode,
                data: data[offset..end].to_vec(),
            });
        }
        Ok(Mar {
            channel,
            version,
            entries,
            signatures,
        })
    }

    pub fn read(path: &Path) -> Result<Mar> {
        let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Mar::parse(&data).with_context(|| format!("parsing {}", path.display()))
    }

    /// Serialize with the signatures in `self.signatures` (their lengths
    /// fix the layout; the bytes may be placeholders).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        ensure!(
            self.channel.len() <= MAX_CHANNEL_LEN && self.version.len() <= MAX_VERSION_LEN,
            "MAR channel or version is too long"
        );
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&[0; 4]); // index offset, filled in below
        out.extend_from_slice(&[0; 8]); // file size, filled in below
        out.extend_from_slice(&(self.signatures.len() as u32).to_be_bytes());
        for sig in &self.signatures {
            out.extend_from_slice(&sig.algorithm.to_be_bytes());
            out.extend_from_slice(&(sig.bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(&sig.bytes);
        }
        out.extend_from_slice(&1u32.to_be_bytes());
        let block_start = out.len();
        out.extend_from_slice(&PRODUCT_INFO_BLOCK_SIZE.to_be_bytes());
        out.extend_from_slice(&PRODUCT_INFO_BLOCK_ID.to_be_bytes());
        out.extend_from_slice(self.channel.as_bytes());
        out.push(0);
        out.extend_from_slice(self.version.as_bytes());
        out.push(0);
        out.resize(block_start + PRODUCT_INFO_BLOCK_SIZE as usize, 0);

        let mut index = Vec::new();
        for entry in &self.entries {
            let offset = u32::try_from(out.len()).context("MAR is too large")?;
            out.extend_from_slice(&entry.data);
            index.extend_from_slice(&offset.to_be_bytes());
            index.extend_from_slice(&(entry.data.len() as u32).to_be_bytes());
            index.extend_from_slice(&entry.mode.to_be_bytes());
            index.extend_from_slice(entry.name.as_bytes());
            index.push(0);
        }
        let index_offset = u32::try_from(out.len()).context("MAR is too large")?;
        out.extend_from_slice(&(index.len() as u32).to_be_bytes());
        out.extend_from_slice(&index);
        let size = out.len() as u64;
        out[4..8].copy_from_slice(&index_offset.to_be_bytes());
        out[8..16].copy_from_slice(&size.to_be_bytes());
        Ok(out)
    }
}

/// The bytes a signature of the serialized MAR `data` covers: everything
/// but the signatures themselves.
pub fn signed_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader { data, pos: 16 };
    let count = r.u32()?;
    ensure!(count <= MAX_SIGNATURES, "too many signatures");
    let mut out = data[..20].to_vec();
    for _ in 0..count {
        out.extend_from_slice(r.take(8)?);
        let len = u32::from_be_bytes(out[out.len() - 4..].try_into().unwrap());
        ensure!(len <= MAX_SIGNATURE_LENGTH, "signature is too long");
        r.take(len as usize)?;
    }
    out.extend_from_slice(&data[r.pos..]);
    Ok(out)
}

fn openssl(args: &[&std::ffi::OsStr]) -> Result<std::process::Output> {
    let output = Command::new("openssl")
        .args(args)
        .output()
        .context("running openssl (is it installed?)")?;
    Ok(output)
}

/// RSA-PKCS#1 v1.5 SHA-384 signature of `data` with the PEM private key.
fn rsa_sign(key: &Path, data: &[u8], work: &Path) -> Result<Vec<u8>> {
    let input = work.join("to-sign");
    let output = work.join("signature");
    fs::write(&input, data)?;
    let result = openssl(&[
        "dgst".as_ref(),
        "-sha384".as_ref(),
        "-sign".as_ref(),
        key.as_os_str(),
        "-out".as_ref(),
        output.as_os_str(),
        input.as_os_str(),
    ])?;
    let _ = fs::remove_file(&input);
    if !result.status.success() {
        bail!(
            "signing with {} failed: {}",
            key.display(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let signature = fs::read(&output)?;
    fs::remove_file(&output)?;
    Ok(signature)
}

/// Whether `signature` is a valid RSA-PKCS#1 v1.5 SHA-384 signature of
/// `data` by the key of the DER certificate `cert`.
fn rsa_verify(cert: &Path, data: &[u8], signature: &[u8], work: &Path) -> Result<bool> {
    let public = work.join("public.pem");
    let result = openssl(&[
        "x509".as_ref(),
        "-inform".as_ref(),
        "DER".as_ref(),
        "-in".as_ref(),
        cert.as_os_str(),
        "-pubkey".as_ref(),
        "-noout".as_ref(),
        "-out".as_ref(),
        public.as_os_str(),
    ])?;
    if !result.status.success() {
        bail!(
            "{} is not a DER certificate: {}",
            cert.display(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let input = work.join("signed");
    let sig = work.join("signature");
    fs::write(&input, data)?;
    fs::write(&sig, signature)?;
    let result = openssl(&[
        "dgst".as_ref(),
        "-sha384".as_ref(),
        "-verify".as_ref(),
        public.as_os_str(),
        "-signature".as_ref(),
        sig.as_os_str(),
        input.as_os_str(),
    ])?;
    for file in [&public, &input, &sig] {
        let _ = fs::remove_file(file);
    }
    Ok(result.status.success())
}

/// A private scratch directory for OpenSSL's input and output files.
fn scratch_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "hallow-mar-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

/// Replace any signatures of `mar` with one made with `key`, and check it
/// verifies against `cert` (so a key and certificate that do not belong
/// together are caught here, not on users' machines).
pub fn sign(mar: &Mar, key: &Path, cert: &Path) -> Result<Vec<u8>> {
    let work = scratch_dir()?;
    let result = (|| {
        // The signature length is part of the signed bytes; learn it from
        // the key first.
        let probe = rsa_sign(key, b"length probe", &work)?;
        let mut signed = mar.clone();
        signed.signatures = vec![Signature {
            algorithm: SIGNATURE_ALGORITHM,
            bytes: vec![0; probe.len()],
        }];
        let mut data = signed.to_bytes()?;
        let signature = rsa_sign(key, &signed_bytes(&data)?, &work)?;
        ensure!(signature.len() == probe.len(), "signature length changed");
        // The signature directly follows the 28-byte header.
        data[28..28 + signature.len()].copy_from_slice(&signature);
        ensure!(
            verify_bytes(&data, cert, &work)?,
            "the MAR signed with {} does not verify against {}; \
             do the key and certificate belong together?",
            key.display(),
            cert.display()
        );
        Ok(data)
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

fn verify_bytes(data: &[u8], cert: &Path, work: &Path) -> Result<bool> {
    let mar = Mar::parse(data)?;
    // The updater requires exactly one signature, made with the one
    // algorithm it knows.
    if mar.signatures.len() != 1 || mar.signatures[0].algorithm != SIGNATURE_ALGORITHM {
        return Ok(false);
    }
    rsa_verify(cert, &signed_bytes(data)?, &mar.signatures[0].bytes, work)
}

/// Whether the MAR file at `path` carries a valid signature by the key of
/// `cert`, checked as Gecko's updater does.
pub fn verify(path: &Path, cert: &Path) -> Result<bool> {
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let work = scratch_dir()?;
    let result = verify_bytes(&data, cert, &work);
    let _ = fs::remove_dir_all(&work);
    result
}

/// What an update manifest (`update-win64.xml`) offers.
pub struct UpdateOffer<'a> {
    /// Hallow version shown to the user (`157.0-8`).
    pub display_version: &'a str,
    /// Gecko application version (`157.0`).
    pub app_version: &'a str,
    /// Build ID of the offered build (`YYYYMMDDhhmmss`).
    pub build_id: &'a str,
    /// Release notes.
    pub details_url: &'a str,
    /// Where the complete MAR is downloaded from (HTTPS).
    pub mar_url: &'a str,
    pub mar_size: u64,
    pub mar_sha512: &'a str,
}

fn xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The update manifest Gecko's update service reads: one complete update,
/// or none.
pub fn update_xml(offer: Option<&UpdateOffer>) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<updates>\n");
    if let Some(o) = offer {
        xml.push_str(&format!(
            "  <update type=\"minor\" displayVersion=\"{}\" appVersion=\"{}\" \
             platformVersion=\"{}\" buildID=\"{}\" detailsURL=\"{}\">\n    \
             <patch type=\"complete\" URL=\"{}\" size=\"{}\" \
             hashFunction=\"sha512\" hashValue=\"{}\"/>\n  </update>\n",
            xml_attr(o.display_version),
            xml_attr(o.app_version),
            xml_attr(o.app_version),
            xml_attr(o.build_id),
            xml_attr(o.details_url),
            xml_attr(o.mar_url),
            o.mar_size,
            xml_attr(o.mar_sha512),
        ));
    }
    xml.push_str("</updates>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_manifest() {
        let offer = UpdateOffer {
            display_version: "157.0-8",
            app_version: "157.0",
            build_id: "20261003120000",
            details_url: "https://example.org/releases?a=1&b=2",
            mar_url: "https://example.org/hallow.mar",
            mar_size: 1234,
            mar_sha512: "ab",
        };
        let xml = update_xml(Some(&offer));
        assert!(xml.contains("appVersion=\"157.0\" platformVersion=\"157.0\""));
        assert!(xml.contains("buildID=\"20261003120000\""));
        assert!(xml.contains("detailsURL=\"https://example.org/releases?a=1&amp;b=2\""));
        assert!(xml.contains(
            "<patch type=\"complete\" URL=\"https://example.org/hallow.mar\" size=\"1234\""
        ));
        assert_eq!(xml.matches("<update ").count(), 1);
        assert_eq!(update_xml(None).matches("<update ").count(), 0);
    }

    fn sample() -> Mar {
        Mar {
            channel: "hallow-release".into(),
            version: "157.0".into(),
            entries: vec![
                Entry {
                    name: "updatev3.manifest".into(),
                    mode: 0o644,
                    data: b"type \"complete\"\n".to_vec(),
                },
                Entry {
                    name: "hallow.exe".into(),
                    mode: 0o755,
                    data: vec![0x4d, 0x5a, 1, 2, 3],
                },
            ],
            signatures: vec![],
        }
    }

    #[test]
    fn round_trip() {
        let mar = sample();
        let data = mar.to_bytes().unwrap();
        assert_eq!(&data[..4], b"MAR1");
        assert_eq!(Mar::parse(&data).unwrap(), mar);

        let mut signed = mar.clone();
        signed.signatures = vec![Signature {
            algorithm: 2,
            bytes: vec![7; 512],
        }];
        let data = signed.to_bytes().unwrap();
        let parsed = Mar::parse(&data).unwrap();
        assert_eq!(parsed, signed);
        // The signed bytes are the file without the signature.
        let covered = signed_bytes(&data).unwrap();
        assert_eq!(covered.len(), data.len() - 512);
        assert_eq!(&covered[..28], &data[..28]);
        assert_eq!(&covered[28..], &data[28 + 512..]);
    }

    #[test]
    fn layout_matches_libmar() {
        let data = sample().to_bytes().unwrap();
        // No signatures, one additional block of 104 bytes right after.
        assert_eq!(&data[16..20], &0u32.to_be_bytes());
        assert_eq!(&data[20..24], &1u32.to_be_bytes());
        assert_eq!(&data[24..28], &104u32.to_be_bytes());
        assert_eq!(&data[28..32], &1u32.to_be_bytes());
        assert_eq!(&data[32..47], b"hallow-release\0");
        assert_eq!(&data[47..53], b"157.0\0");
        // The first file starts after the block.
        let index_offset = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
        let first =
            u32::from_be_bytes(data[index_offset + 4..index_offset + 8].try_into().unwrap());
        assert_eq!(first, 24 + 104);
    }

    #[test]
    fn rejects_damaged_files() {
        let data = sample().to_bytes().unwrap();
        assert!(Mar::parse(&data[..data.len() - 1]).is_err());
        let mut bad = data.clone();
        bad[0] = b'X';
        assert!(Mar::parse(&bad).is_err());
        let mut bad = data.clone();
        bad[4..8].copy_from_slice(&5000u32.to_be_bytes());
        assert!(Mar::parse(&bad).is_err());
    }

    fn openssl_available() -> bool {
        Command::new("openssl").arg("version").output().is_ok()
    }

    fn make_key(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
        let key = dir.join(format!("{name}.pem"));
        let cert = dir.join(format!("{name}.der"));
        let status = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            ])
            .args(["-subj", "/CN=Hallow test"])
            .arg("-keyout")
            .arg(&key)
            .args(["-outform", "DER", "-out"])
            .arg(&cert)
            .output()
            .unwrap();
        assert!(status.status.success());
        (key, cert)
    }

    #[test]
    fn sign_and_verify() {
        if !openssl_available() {
            eprintln!("openssl not found; skipping");
            return;
        }
        let dir = scratch_dir().unwrap();
        let (key, cert) = make_key(&dir, "good");
        let (_, other) = make_key(&dir, "other");
        let path = dir.join("signed.mar");
        let data = sign(&sample(), &key, &cert).unwrap();
        fs::write(&path, &data).unwrap();
        assert!(verify(&path, &cert).unwrap());
        assert!(!verify(&path, &other).unwrap());
        let parsed = Mar::read(&path).unwrap();
        assert_eq!(parsed.signatures.len(), 1);
        assert_eq!(parsed.signatures[0].algorithm, 2);
        assert_eq!(parsed.signatures[0].bytes.len(), 256);

        // Any change to the content breaks the signature.
        let mut tampered = data.clone();
        let last = tampered.len() - 30;
        tampered[last] ^= 1;
        fs::write(&path, &tampered).unwrap();
        assert!(!verify(&path, &cert).unwrap());

        // Re-signing replaces the old signature.
        let resigned = sign(&parsed, &key, &cert).unwrap();
        assert_eq!(resigned.len(), data.len());

        // A key and certificate that do not belong together are refused.
        assert!(sign(&sample(), &key, &other).is_err());

        // Unsigned MARs never verify.
        fs::write(&path, sample().to_bytes().unwrap()).unwrap();
        assert!(!verify(&path, &cert).unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }
}
