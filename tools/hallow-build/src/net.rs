//! Small HTTP helpers on top of ureq (rustls, system trust store, proxy from env).

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha512};
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use crate::util::{hex, human_bytes};

fn agent() -> Agent {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .user_agent(concat!("hallow-build/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

pub fn get_string(url: &str) -> Result<String> {
    let mut response = agent()
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    response
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))
}

/// Stream `url` into `dest`, returning the SHA-512 of what was written.
/// Writes to `<dest>.part` first so an interrupted download is never mistaken
/// for a complete one. Retries a few times on network errors.
pub fn download(url: &str, dest: &Path) -> Result<String> {
    let mut last_error = None;
    for attempt in 1..=4 {
        match try_download(url, dest) {
            Ok(hash) => return Ok(hash),
            Err(err) => {
                eprintln!("download attempt {attempt} failed: {err:#}");
                last_error = Some(err);
                std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
            }
        }
    }
    Err(last_error.unwrap())
}

fn try_download(url: &str, dest: &Path) -> Result<String> {
    let response = agent()
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();

    let part = dest.with_extension("part");
    let mut out = BufWriter::new(File::create(&part)?);
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done: u64 = 0;
    let mut next_report: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        if done >= next_report {
            match total {
                Some(total) => eprintln!(
                    "  {} / {} ({}%)",
                    human_bytes(done),
                    human_bytes(total),
                    done * 100 / total.max(1)
                ),
                None => eprintln!("  {}", human_bytes(done)),
            }
            next_report = done + (64 << 20);
        }
    }
    out.flush()?;
    drop(out);
    if let Some(total) = total
        && done != total
    {
        bail!("short read: got {done} of {total} bytes");
    }
    fs::rename(&part, dest)?;
    Ok(hex(&hasher.finalize()))
}
