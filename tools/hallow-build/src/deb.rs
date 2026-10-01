//! Build the Hallow `.deb` without dpkg: an `ar` archive holding
//! `debian-binary`, `control.tar.xz` and `data.tar.xz`, compressed with the
//! pure-Rust xz encoder. Dependencies are derived from the ELF files that
//! were actually built.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::num::NonZeroU64;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use lzma_rust2::{XzOptions, XzWriter, XzWriterMt};
use md5::{Digest, Md5};

use crate::config::{APP_NAME, Context};
use crate::{elf, fetch, icons};

const ARCH: &str = "amd64";
const INSTALL_DIR: &str = "usr/lib/hallow";

/// Shared libraries Firefox links against, mapped to Debian dependencies.
/// The `t64` names are Debian 13 / Ubuntu 24.04+, the others older releases.
const SONAME_PACKAGES: &[(&str, &str)] = &[
    ("ld-linux-x86-64.so.2", "libc6"),
    ("libanl.so.1", "libc6"),
    ("libc.so.6", "libc6"),
    ("libdl.so.2", "libc6"),
    ("libm.so.6", "libc6"),
    ("libpthread.so.0", "libc6"),
    ("libresolv.so.2", "libc6"),
    ("librt.so.1", "libc6"),
    ("libutil.so.1", "libc6"),
    ("libgcc_s.so.1", "libgcc-s1"),
    ("libstdc++.so.6", "libstdc++6"),
    ("libasound.so.2", "libasound2t64 | libasound2"),
    ("libatk-1.0.so.0", "libatk1.0-0t64 | libatk1.0-0"),
    ("libcairo-gobject.so.2", "libcairo-gobject2"),
    ("libcairo.so.2", "libcairo2"),
    ("libdbus-1.so.3", "libdbus-1-3"),
    ("libdbus-glib-1.so.2", "libdbus-glib-1-2"),
    ("libdrm.so.2", "libdrm2"),
    ("libfontconfig.so.1", "libfontconfig1"),
    ("libfreetype.so.6", "libfreetype6"),
    ("libgbm.so.1", "libgbm1"),
    ("libgdk-3.so.0", "libgtk-3-0t64 | libgtk-3-0"),
    (
        "libgdk_pixbuf-2.0.so.0",
        "libgdk-pixbuf-2.0-0 | libgdk-pixbuf2.0-0",
    ),
    ("libgio-2.0.so.0", "libglib2.0-0t64 | libglib2.0-0"),
    ("libglib-2.0.so.0", "libglib2.0-0t64 | libglib2.0-0"),
    ("libgmodule-2.0.so.0", "libglib2.0-0t64 | libglib2.0-0"),
    ("libgobject-2.0.so.0", "libglib2.0-0t64 | libglib2.0-0"),
    ("libgthread-2.0.so.0", "libglib2.0-0t64 | libglib2.0-0"),
    ("libgtk-3.so.0", "libgtk-3-0t64 | libgtk-3-0"),
    ("libharfbuzz.so.0", "libharfbuzz0b"),
    ("libpango-1.0.so.0", "libpango-1.0-0"),
    ("libpangocairo-1.0.so.0", "libpangocairo-1.0-0"),
    ("libpangoft2-1.0.so.0", "libpangoft2-1.0-0"),
    ("libX11-xcb.so.1", "libx11-xcb1"),
    ("libX11.so.6", "libx11-6"),
    ("libxcb-shm.so.0", "libxcb-shm0"),
    ("libxcb.so.1", "libxcb1"),
    ("libXcomposite.so.1", "libxcomposite1"),
    ("libXcursor.so.1", "libxcursor1"),
    ("libXdamage.so.1", "libxdamage1"),
    ("libXext.so.6", "libxext6"),
    ("libXfixes.so.3", "libxfixes3"),
    ("libXi.so.6", "libxi6"),
    ("libxkbcommon.so.0", "libxkbcommon0"),
    ("libXrandr.so.2", "libxrandr2"),
    ("libXrender.so.1", "libxrender1"),
    ("libXtst.so.6", "libxtst6"),
    ("libz.so.1", "zlib1g"),
];

/// Libraries Gecko loads at runtime with dlopen() for optional features.
const RECOMMENDS: &str = "libpci3, libegl1, libpulse0, libva2, libva-drm2, \
     libavcodec62 | libavcodec61 | libavcodec60 | libavcodec59 | libavcodec58 | libavcodec-extra";

const DESCRIPTION: &str = "Clean, lightweight web browser built on Gecko
 Hallow is a Firefox fork with a minimal interface, no telemetry, no
 sponsored content and Rust-first engine settings, built from the latest
 Firefox release.";

enum Source {
    Dir,
    Disk { path: PathBuf, size: u64, mode: u32 },
    Bytes { data: Vec<u8>, mode: u32 },
    Symlink(String),
}

/// The filesystem tree of the package, keyed by path without a leading `/`.
#[derive(Default)]
struct Tree(BTreeMap<String, Source>);

impl Tree {
    fn add(&mut self, path: &str, source: Source) {
        let mut parent = Path::new(path).parent();
        while let Some(dir) = parent.filter(|p| !p.as_os_str().is_empty()) {
            self.0
                .entry(dir.to_string_lossy().into_owned())
                .or_insert(Source::Dir);
            parent = dir.parent();
        }
        self.0.insert(path.to_string(), source);
    }

    fn add_bytes(&mut self, path: &str, data: impl Into<Vec<u8>>, mode: u32) {
        self.add(
            path,
            Source::Bytes {
                data: data.into(),
                mode,
            },
        );
    }

    /// Add `dir` (recursively) under `prefix`.
    fn add_disk_dir(&mut self, prefix: &str, dir: &Path) -> Result<()> {
        self.add(prefix, Source::Dir);
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&path)?.to_string_lossy().into_owned();
                self.add(&name, Source::Symlink(target));
            } else if meta.is_dir() {
                self.add_disk_dir(&name, &path)?;
            } else {
                let executable = meta.permissions().mode() & 0o111 != 0;
                self.add(
                    &name,
                    Source::Disk {
                        path,
                        size: meta.len(),
                        mode: if executable { 0o755 } else { 0o644 },
                    },
                );
            }
        }
        Ok(())
    }

    /// Installed-Size in KiB, computed the way dpkg-gencontrol does.
    fn installed_size_kib(&self) -> u64 {
        self.0
            .values()
            .map(|s| match s {
                Source::Disk { size, .. } => size.div_ceil(1024),
                Source::Bytes { data, .. } => (data.len() as u64).div_ceil(1024),
                Source::Dir | Source::Symlink(_) => 1,
            })
            .sum()
    }
}

pub fn run(ctx: &Context, input: Option<PathBuf>, output_dir: Option<PathBuf>) -> Result<PathBuf> {
    let app_dir = resolve_input(ctx, input)?;
    let version = ctx.config.package_version();
    check_application_ini(&app_dir, &ctx.config.firefox.version)?;

    let packaging = ctx.root.join("packaging");
    let mut tree = Tree::default();
    tree.add_disk_dir(INSTALL_DIR, &app_dir)?;
    // Tells Gecko it is managed by a package manager (hides update UI etc.).
    tree.add_bytes(
        &format!("{INSTALL_DIR}/is-packaged-app"),
        "This is a packaged app.\n",
        0o644,
    );
    tree.add(
        "usr/bin/hallow",
        Source::Symlink("../lib/hallow/hallow".into()),
    );
    tree.add_bytes(
        "usr/share/applications/hallow.desktop",
        fs::read(packaging.join("hallow.desktop"))?,
        0o644,
    );
    let logo = icons::load_logo(&ctx.root.join("branding"))?;
    for size in icons::ICON_SIZES {
        tree.add_bytes(
            &format!("usr/share/icons/hicolor/{size}x{size}/apps/hallow.png"),
            icons::render_png(&logo, size, size)?,
            0o644,
        );
    }
    tree.add_bytes(
        "usr/share/icons/hicolor/scalable/apps/hallow.svg",
        logo,
        0o644,
    );
    tree.add_bytes(
        "usr/share/doc/hallow/copyright",
        fs::read(packaging.join("copyright"))?,
        0o644,
    );

    let depends = depends(&app_dir)?;
    let control = control_file(ctx, &version, &depends, tree.installed_size_kib());
    eprintln!("{control}");

    let mut scripts = Vec::new();
    for name in ["postinst", "prerm", "postrm"] {
        scripts.push((name, fs::read(packaging.join(name))?));
    }

    let output_dir = output_dir.unwrap_or_else(|| ctx.root.join("dist"));
    fs::create_dir_all(&output_dir)?;
    let output = output_dir.join(format!("{APP_NAME}_{version}_{ARCH}.deb"));
    write_deb(&output, &control, &scripts, &tree, mtime())?;
    eprintln!(
        "wrote {} ({})",
        output.display(),
        crate::util::human_bytes(fs::metadata(&output)?.len())
    );
    Ok(output)
}

/// Accept the staged `dist/hallow` directory or the `.tar.xz` from `mach package`.
fn resolve_input(ctx: &Context, input: Option<PathBuf>) -> Result<PathBuf> {
    let input = input.unwrap_or_else(|| ctx.objdir().join("dist").join(APP_NAME));
    if input.is_dir() {
        if !input.join(APP_NAME).exists() {
            bail!("{} has no `{APP_NAME}` binary", input.display());
        }
        return Ok(input);
    }
    if input.to_string_lossy().ends_with(".tar.xz") {
        let unpack = ctx.work.join("deb-input");
        let _ = fs::remove_dir_all(&unpack);
        fs::create_dir_all(&unpack)?;
        fetch::extract_tar_xz(&input, &unpack)?;
        return resolve_input(ctx, Some(unpack.join(APP_NAME)));
    }
    bail!(
        "{} is neither a packaged app directory nor a .tar.xz",
        input.display()
    )
}

fn check_application_ini(app_dir: &Path, firefox_version: &str) -> Result<()> {
    let ini = fs::read_to_string(app_dir.join("application.ini"))
        .context("reading application.ini from the packaged app")?;
    let version = ini
        .lines()
        .find_map(|l| l.strip_prefix("Version="))
        .context("application.ini has no Version")?;
    if version.trim() != firefox_version {
        bail!("packaged app is version {version}, but hallow.toml pins {firefox_version}");
    }
    Ok(())
}

fn control_file(ctx: &Context, version: &str, depends: &[String], installed_kib: u64) -> String {
    format!(
        "Package: {APP_NAME}\n\
         Version: {version}\n\
         Architecture: {ARCH}\n\
         Maintainer: {maintainer}\n\
         Installed-Size: {installed_kib}\n\
         Depends: {depends}\n\
         Recommends: {RECOMMENDS}\n\
         Provides: gnome-www-browser, www-browser\n\
         Section: web\n\
         Priority: optional\n\
         Homepage: {homepage}\n\
         Description: {DESCRIPTION}\n",
        maintainer = ctx.config.package.maintainer,
        homepage = ctx.config.package.homepage,
        depends = depends.join(", "),
    )
}

/// Debian dependencies for every shared library the packaged ELF files need
/// that the package does not ship itself.
fn depends(app_dir: &Path) -> Result<Vec<String>> {
    let mut provided = BTreeSet::new();
    let mut needed = BTreeSet::new();
    let mut stack = vec![app_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && elf::is_elf(&path) {
                let dynamic = elf::dynamic(&path)?;
                provided.insert(entry.file_name().to_string_lossy().into_owned());
                provided.extend(dynamic.soname);
                needed.extend(dynamic.needed);
            }
        }
    }
    if needed.is_empty() {
        bail!(
            "found no dynamically linked ELF files in {}",
            app_dir.display()
        );
    }

    let table: BTreeMap<&str, &str> = SONAME_PACKAGES.iter().copied().collect();
    let mut deps = BTreeSet::new();
    let mut unknown = Vec::new();
    for soname in needed.difference(&provided) {
        match table.get(soname.as_str()) {
            Some(dep) => {
                deps.insert(dep.to_string());
            }
            None => match dpkg_owner(soname) {
                Some(dep) => {
                    eprintln!("warning: {soname} is not in SONAME_PACKAGES; dpkg says {dep}");
                    deps.insert(dep);
                }
                None => unknown.push(soname.clone()),
            },
        }
    }
    if !unknown.is_empty() {
        bail!(
            "no Debian package known for {}; add them to SONAME_PACKAGES in deb.rs",
            unknown.join(", ")
        );
    }
    Ok(deps.into_iter().collect())
}

/// Ask the build host's dpkg which package ships `soname`.
fn dpkg_owner(soname: &str) -> Option<String> {
    let output = Command::new("dpkg-query")
        .args(["-S", &format!("*/{soname}")])
        .output()
        .ok()?;
    owner_from_dpkg_query(&String::from_utf8_lossy(&output.stdout))
}

/// Pick the package that ships the library for our architecture from
/// `dpkg-query -S` output (`pkg[:arch][, pkg...]: /path`). Multilib copies
/// such as `libc6-i386: /lib32/...` must not become dependencies.
fn owner_from_dpkg_query(stdout: &str) -> Option<String> {
    let line = stdout
        .lines()
        .find(|line| line.contains("/x86_64-linux-gnu/"))?;
    let (packages, _path) = line.split_once(": ")?;
    let package = packages.split(',').next()?.split(':').next()?.trim();
    if package.is_empty() {
        return None;
    }
    Some(match package.strip_suffix("t64") {
        Some(old) => format!("{package} | {old}"),
        None => package.to_string(),
    })
}

fn mtime() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
}

fn tar_header(kind: tar::EntryType, mode: u32, size: u64, mtime: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(size);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(mtime);
    // Both only fail for names longer than the field; "root" always fits.
    let _ = header.set_username("root");
    let _ = header.set_groupname("root");
    header
}

/// Debian packages name members `./usr/...`, which the tar crate's path
/// helpers would normalize away, so names are written into the header
/// directly, with GNU long-name records for anything over 100 bytes.
fn append_entry<W: Write>(
    tar: &mut tar::Builder<W>,
    mut header: tar::Header,
    name: &str,
    link: Option<&str>,
    data: impl Read,
) -> Result<()> {
    for (value, kind) in [
        (Some(name), tar::EntryType::GNULongName),
        (link, tar::EntryType::GNULongLink),
    ] {
        let Some(value) = value.filter(|v| v.len() >= 100) else {
            continue;
        };
        let mut long = tar_header(kind, 0o644, value.len() as u64 + 1, 0);
        long.as_old_mut().name[..13].copy_from_slice(b"././@LongLink");
        long.set_cksum();
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        tar.append(&long, &bytes[..])?;
    }
    let old = header.as_old_mut();
    let n = name.len().min(100);
    old.name[..n].copy_from_slice(&name.as_bytes()[..n]);
    if let Some(link) = link {
        let n = link.len().min(100);
        old.linkname[..n].copy_from_slice(&link.as_bytes()[..n]);
    }
    header.set_cksum();
    tar.append(&header, data)?;
    Ok(())
}

/// Write `tree` as a tar stream, returning the md5sums of regular files.
fn write_data_tar<W: Write>(out: W, tree: &Tree, mtime: u64) -> Result<(W, String)> {
    let mut tar = tar::Builder::new(out);
    let mut md5sums = String::new();
    let dir = |mtime| tar_header(tar::EntryType::Directory, 0o755, 0, mtime);
    append_entry(&mut tar, dir(mtime), "./", None, io::empty())?;

    for (path, source) in &tree.0 {
        let name = format!("./{path}");
        match source {
            Source::Dir => {
                append_entry(&mut tar, dir(mtime), &format!("{name}/"), None, io::empty())?;
            }
            Source::Symlink(target) => {
                let header = tar_header(tar::EntryType::Symlink, 0o777, 0, mtime);
                append_entry(&mut tar, header, &name, Some(target), io::empty())?;
            }
            Source::Bytes { data, mode } => {
                md5sums.push_str(&format!("{}  {path}\n", md5_hex(&mut &data[..])?));
                let header = tar_header(tar::EntryType::Regular, *mode, data.len() as u64, mtime);
                append_entry(&mut tar, header, &name, None, &data[..])?;
            }
            Source::Disk {
                path: file,
                size,
                mode,
            } => {
                md5sums.push_str(&format!("{}  {path}\n", md5_hex(&mut File::open(file)?)?));
                let header = tar_header(tar::EntryType::Regular, *mode, *size, mtime);
                let reader = BufReader::with_capacity(1 << 20, File::open(file)?);
                append_entry(&mut tar, header, &name, None, reader.take(*size))?;
            }
        }
    }
    Ok((tar.into_inner()?, md5sums))
}

fn md5_hex(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::util::hex(&hasher.finalize()))
}

fn write_deb(
    output: &Path,
    control: &str,
    scripts: &[(&str, Vec<u8>)],
    tree: &Tree,
    mtime: u64,
) -> Result<()> {
    // data.tar.xz: compressed in parallel 32 MiB blocks.
    let data_path = output.with_extension("data.tar.xz");
    let mut options = XzOptions::with_preset(6);
    options.set_block_size(NonZeroU64::new(32 << 20));
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get() as u32);
    let xz = XzWriterMt::new(BufWriter::new(File::create(&data_path)?), options, workers)?;
    let (xz, md5sums) = write_data_tar(xz, tree, mtime)?;
    xz.finish()?.flush()?;

    // control.tar.xz
    let mut control_tree = Tree::default();
    control_tree.add_bytes("control", control, 0o644);
    control_tree.add_bytes("md5sums", md5sums, 0o644);
    for (name, body) in scripts {
        control_tree.add_bytes(name, body.clone(), 0o755);
    }
    let xz = XzWriter::new(Vec::new(), XzOptions::with_preset(9))?;
    let (xz, _) = write_data_tar(xz, &control_tree, mtime)?;
    let control_tar = xz.finish()?;

    let mut out = BufWriter::new(File::create(output)?);
    out.write_all(b"!<arch>\n")?;
    ar_member(&mut out, "debian-binary", mtime, 4, &mut &b"2.0\n"[..])?;
    ar_member(
        &mut out,
        "control.tar.xz",
        mtime,
        control_tar.len() as u64,
        &mut &control_tar[..],
    )?;
    let data_len = fs::metadata(&data_path)?.len();
    ar_member(
        &mut out,
        "data.tar.xz",
        mtime,
        data_len,
        &mut File::open(&data_path)?,
    )?;
    out.flush()?;
    fs::remove_file(&data_path)?;
    Ok(())
}

/// One member of a System V `ar` archive, as read by dpkg.
fn ar_member(
    out: &mut impl Write,
    name: &str,
    mtime: u64,
    size: u64,
    data: &mut impl Read,
) -> Result<()> {
    let header = format!(
        "{name:<16}{mtime:<12}{:<6}{:<6}{:<8}{size:<10}`\n",
        0, 0, "100644"
    );
    assert_eq!(header.len(), 60, "ar header for {name}");
    out.write_all(header.as_bytes())?;
    let copied = io::copy(data, out)?;
    if copied != size {
        bail!("ar member {name}: wrote {copied} bytes, expected {size}");
    }
    if size % 2 == 1 {
        out.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_adds_parent_directories() {
        let mut tree = Tree::default();
        tree.add_bytes("usr/share/doc/hallow/copyright", "x", 0o644);
        let paths: Vec<_> = tree.0.keys().cloned().collect();
        assert_eq!(
            paths,
            [
                "usr",
                "usr/share",
                "usr/share/doc",
                "usr/share/doc/hallow",
                "usr/share/doc/hallow/copyright"
            ]
        );
        assert_eq!(tree.installed_size_kib(), 5);
    }

    #[test]
    fn dpkg_owner_ignores_other_architectures() {
        let out = "libc6-i386: /lib32/libresolv.so.2\nlibc6:amd64: /lib/x86_64-linux-gnu/libresolv.so.2\n";
        assert_eq!(owner_from_dpkg_query(out).as_deref(), Some("libc6"));
        let out = "libfoo1t64:amd64: /usr/lib/x86_64-linux-gnu/libfoo.so.1\n";
        assert_eq!(
            owner_from_dpkg_query(out).as_deref(),
            Some("libfoo1t64 | libfoo1")
        );
        assert_eq!(
            owner_from_dpkg_query("libc6-i386: /lib32/libx.so.1\n"),
            None
        );
        assert_eq!(owner_from_dpkg_query(""), None);
    }

    #[test]
    fn soname_table_is_sorted_and_unique() {
        let names: Vec<_> = SONAME_PACKAGES.iter().map(|(s, _)| *s).collect();
        let unique: BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    /// Build a small package and have the real dpkg-deb inspect it.
    #[test]
    fn dpkg_accepts_our_archives() {
        if Command::new("dpkg-deb").arg("--version").output().is_err() {
            eprintln!("dpkg-deb not installed; skipping");
            return;
        }
        let tmp = std::env::temp_dir().join(format!("hallow-deb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("app/sub")).unwrap();
        fs::write(tmp.join("app/odd"), "odd length!").unwrap();
        fs::write(tmp.join("app/sub/run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(
            tmp.join("app/sub/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let long = "a-very-long-directory-name-that-needs-gnu-long-name-records".repeat(3);
        fs::create_dir_all(tmp.join("app").join(&long)).unwrap();
        fs::write(tmp.join("app").join(&long).join("f"), "x").unwrap();

        let mut tree = Tree::default();
        tree.add_disk_dir("usr/lib/test", &tmp.join("app")).unwrap();
        tree.add(
            "usr/bin/test",
            Source::Symlink("../lib/test/sub/run.sh".into()),
        );
        let control = "Package: hallow-test\nVersion: 1.0-1\nArchitecture: all\n\
                       Maintainer: Test <t@example.com>\nDescription: test\n test package\n";
        let deb = tmp.join("test.deb");
        write_deb(
            &deb,
            control,
            &[("postinst", b"#!/bin/sh\nexit 0\n".to_vec())],
            &tree,
            1_700_000_000,
        )
        .unwrap();

        let info = Command::new("dpkg-deb")
            .arg("--info")
            .arg(&deb)
            .output()
            .unwrap();
        assert!(
            info.status.success(),
            "{}",
            String::from_utf8_lossy(&info.stderr)
        );
        let info = String::from_utf8_lossy(&info.stdout);
        assert!(info.contains("Package: hallow-test"), "{info}");
        assert!(info.contains("postinst"), "{info}");

        let contents = Command::new("dpkg-deb")
            .arg("--contents")
            .arg(&deb)
            .output()
            .unwrap();
        assert!(
            contents.status.success(),
            "{}",
            String::from_utf8_lossy(&contents.stderr)
        );
        let contents = String::from_utf8_lossy(&contents.stdout);
        assert!(contents.contains("rwxr-xr-x root/root"), "{contents}");
        assert!(
            contents.contains("./usr/bin/test -> ../lib/test/sub/run.sh"),
            "{contents}"
        );
        assert!(contents.contains(&format!("{long}/f")), "{contents}");

        let extract = tmp.join("extract");
        let status = Command::new("dpkg-deb")
            .arg("--extract")
            .arg(&deb)
            .arg(&extract)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(extract.join("usr/lib/test/odd")).unwrap(),
            "odd length!"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }
}
