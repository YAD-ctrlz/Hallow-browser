//! Rewrite the icon and version resources of a Windows executable.
//!
//! Firefox's Windows installer is a prebuilt 7-Zip self-extractor stub
//! (other-licenses/7zstub/firefox/7zSD.Win32.sfx) with the installer
//! appended. The stub carries Firefox's setup icon and "Firefox" /
//! "Mozilla" version strings, which is what Explorer, the Downloads folder
//! and Windows' security prompts show for the file users download. Hallow
//! rewrites them before the installer is assembled.
//!
//! Only executables whose resource section is the last section and is
//! followed by nothing else are supported (the stub is such a file): the
//! section can then be rebuilt and resized in place.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result, bail, ensure};

pub const RT_ICON: u32 = 3;
pub const RT_GROUP_ICON: u32 = 14;
pub const RT_VERSION: u32 = 16;

/// A resource name: a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResId {
    // Named entries sort before numbered ones, as the format requires.
    Name(String),
    Id(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub lang: u32,
    pub codepage: u32,
    pub data: Vec<u8>,
}

/// type → name → languages.
pub type Resources = BTreeMap<ResId, BTreeMap<ResId, Vec<Leaf>>>;

fn u16_at(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        d.get(o..o + 2).context("truncated")?.try_into()?,
    ))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        d.get(o..o + 4).context("truncated")?.try_into()?,
    ))
}

fn put_u16(d: &mut [u8], o: usize, v: u16) {
    d[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

/// Where things are in a PE file.
struct Layout {
    /// Offset of the optional header.
    opt: usize,
    pe32_plus: bool,
    section_alignment: usize,
    file_alignment: usize,
    /// Offset of the resource section's header.
    rsrc_header: usize,
    rsrc_rva: usize,
    rsrc_raw: usize,
}

/// `overlay`: whether data may follow the resource section (reading an
/// installer, which is a stub with an archive appended).
fn layout(file: &[u8], overlay: bool) -> Result<Layout> {
    ensure!(file.get(0..2) == Some(b"MZ"), "not a Windows executable");
    let pe = u32_at(file, 0x3c)? as usize;
    ensure!(file.get(pe..pe + 4) == Some(b"PE\0\0"), "no PE header");
    let sections = u16_at(file, pe + 6)? as usize;
    let opt_size = u16_at(file, pe + 20)? as usize;
    let opt = pe + 24;
    let pe32_plus = match u16_at(file, opt)? {
        0x10b => false,
        0x20b => true,
        m => bail!("unknown optional header magic {m:#x}"),
    };
    let dirs = opt + if pe32_plus { 112 } else { 96 };
    let rsrc_rva = u32_at(file, dirs + 2 * 8)? as usize;
    ensure!(rsrc_rva != 0, "the executable has no resources");
    let table = opt + opt_size;
    let mut found = None;
    let mut last_end = 0;
    for i in 0..sections {
        let h = table + 40 * i;
        let va = u32_at(file, h + 12)? as usize;
        let raw_size = u32_at(file, h + 16)? as usize;
        let raw = u32_at(file, h + 20)? as usize;
        if va == rsrc_rva {
            found = Some((h, raw, raw + raw_size));
        }
        last_end = last_end.max(raw + raw_size);
    }
    let (rsrc_header, rsrc_raw, rsrc_end) = found.context("resource section not found")?;
    ensure!(
        rsrc_end == last_end && (overlay || rsrc_end == file.len()),
        "the resource section is not at the end of the file"
    );
    Ok(Layout {
        opt,
        pe32_plus,
        section_alignment: u32_at(file, opt + 32)? as usize,
        file_alignment: u32_at(file, opt + 36)? as usize,
        rsrc_header,
        rsrc_rva,
        rsrc_raw,
    })
}

fn read_name(sec: &[u8], offset: usize) -> Result<String> {
    let len = u16_at(sec, offset)? as usize;
    let units: Vec<u16> = (0..len)
        .map(|i| u16_at(sec, offset + 2 + 2 * i))
        .collect::<Result<_>>()?;
    String::from_utf16(&units).context("resource name is not UTF-16")
}

fn read_dir(sec: &[u8], offset: usize) -> Result<Vec<(ResId, u32)>> {
    let named = u16_at(sec, offset + 12)? as usize;
    let ids = u16_at(sec, offset + 14)? as usize;
    let mut out = Vec::new();
    for i in 0..named + ids {
        let e = offset + 16 + 8 * i;
        let name = u32_at(sec, e)?;
        let id = if name & 0x8000_0000 != 0 {
            ResId::Name(read_name(sec, (name & 0x7fff_ffff) as usize)?)
        } else {
            ResId::Id(name)
        };
        out.push((id, u32_at(sec, e + 4)?));
    }
    Ok(out)
}

/// `lenient`: skip resources whose data is not in the section (UPX-packed
/// executables keep only some resources there).
fn parse_resources(sec: &[u8], rva: usize, lenient: bool) -> Result<Resources> {
    let mut res = Resources::new();
    for (kind, kdir) in read_dir(sec, 0)? {
        ensure!(kdir & 0x8000_0000 != 0, "malformed resource directory");
        let names = res.entry(kind).or_default();
        for (name, ndir) in read_dir(sec, (kdir & 0x7fff_ffff) as usize)? {
            ensure!(ndir & 0x8000_0000 != 0, "malformed resource directory");
            let leaves = names.entry(name).or_default();
            for (lang, entry) in read_dir(sec, (ndir & 0x7fff_ffff) as usize)? {
                let ResId::Id(lang) = lang else {
                    bail!("named resource language");
                };
                let entry = entry as usize;
                let data_rva = u32_at(sec, entry)? as usize;
                let size = u32_at(sec, entry + 4)? as usize;
                let data = data_rva
                    .checked_sub(rva)
                    .and_then(|start| sec.get(start..start + size));
                let Some(data) = data else {
                    if lenient {
                        continue;
                    }
                    bail!("resource outside its section");
                };
                leaves.push(Leaf {
                    lang,
                    codepage: u32_at(sec, entry + 8)?,
                    data: data.to_vec(),
                });
            }
        }
    }
    Ok(res)
}

/// Lay out a resource section for `rva`: directories, then names, then data
/// entries, then the data.
fn build_resources(res: &Resources, rva: usize) -> Vec<u8> {
    let dir_size = |n: usize| 16 + 8 * n;
    // Offsets of every directory, breadth first.
    let mut offset = dir_size(res.len());
    let mut name_dirs = Vec::new();
    for names in res.values() {
        name_dirs.push(offset);
        offset += dir_size(names.len());
    }
    let mut lang_dirs = Vec::new();
    for names in res.values() {
        for leaves in names.values() {
            lang_dirs.push(offset);
            offset += dir_size(leaves.len());
        }
    }
    // Names.
    let mut strings = BTreeMap::new();
    let mut add_name = |id: &ResId, offset: &mut usize| {
        if let ResId::Name(s) = id
            && !strings.contains_key(s)
        {
            strings.insert(s.clone(), *offset);
            *offset += 2 + 2 * s.encode_utf16().count();
        }
    };
    for (kind, names) in res {
        add_name(kind, &mut offset);
        for name in names.keys() {
            add_name(name, &mut offset);
        }
    }
    offset = align(offset, 4);
    let entries_start = offset;
    let leaf_count: usize = res.values().flat_map(|n| n.values()).map(Vec::len).sum();
    offset += 16 * leaf_count;
    let mut data_offsets = Vec::new();
    for leaf in res.values().flat_map(|n| n.values()).flatten() {
        offset = align(offset, 8);
        data_offsets.push(offset);
        offset += leaf.data.len();
    }
    let mut out = vec![0u8; align(offset, 8)];

    let name_field = |id: &ResId| match id {
        ResId::Name(s) => 0x8000_0000 | strings[s] as u32,
        ResId::Id(n) => *n,
    };
    let write_dir = |out: &mut Vec<u8>, at: usize, ids: Vec<(&ResId, u32)>| {
        let named = ids
            .iter()
            .filter(|(id, _)| matches!(id, ResId::Name(_)))
            .count();
        put_u16(out, at + 12, named as u16);
        put_u16(out, at + 14, (ids.len() - named) as u16);
        for (i, (id, target)) in ids.into_iter().enumerate() {
            put_u32(out, at + 16 + 8 * i, name_field(id));
            put_u32(out, at + 20 + 8 * i, target);
        }
    };
    write_dir(
        &mut out,
        0,
        res.keys()
            .zip(&name_dirs)
            .map(|(k, &o)| (k, 0x8000_0000 | o as u32))
            .collect(),
    );
    let mut lang_dir = lang_dirs.iter();
    let mut leaf_index = 0;
    for (names, &name_dir) in res.values().zip(&name_dirs) {
        let mut ids = Vec::new();
        for (name, leaves) in names {
            let at = *lang_dir.next().unwrap();
            ids.push((name, 0x8000_0000 | at as u32));
            let langs: Vec<ResId> = leaves.iter().map(|l| ResId::Id(l.lang)).collect();
            let mut entries = Vec::new();
            for (lang, leaf) in langs.iter().zip(leaves) {
                let entry = entries_start + 16 * leaf_index;
                let data = data_offsets[leaf_index];
                put_u32(&mut out, entry, (rva + data) as u32);
                put_u32(&mut out, entry + 4, leaf.data.len() as u32);
                put_u32(&mut out, entry + 8, leaf.codepage);
                out[data..data + leaf.data.len()].copy_from_slice(&leaf.data);
                entries.push((lang, entry as u32));
                leaf_index += 1;
            }
            write_dir(&mut out, at, entries);
        }
        write_dir(&mut out, name_dir, ids);
    }
    for (s, &at) in &strings {
        let units: Vec<u16> = s.encode_utf16().collect();
        put_u16(&mut out, at, units.len() as u16);
        for (i, u) in units.iter().enumerate() {
            put_u16(&mut out, at + 2 + 2 * i, *u);
        }
    }
    out
}

/// The standard PE image checksum.
fn checksum(file: &[u8], checksum_offset: usize) -> u32 {
    let mut sum: u64 = 0;
    for (i, chunk) in file.chunks(2).enumerate() {
        if i * 2 == checksum_offset || i * 2 == checksum_offset + 2 {
            continue;
        }
        let word = u16::from_le_bytes([chunk[0], *chunk.get(1).unwrap_or(&0)]) as u64;
        sum += word;
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum = (sum & 0xffff) + (sum >> 16);
    (sum as u32).wrapping_add(file.len() as u32)
}

/// Read the resources of the executable `file` (which may be followed by
/// other data, like an installer's archive, and may be UPX-packed: then
/// only the resources stored unpacked are read).
pub fn read_resources(file: &[u8]) -> Result<Resources> {
    let l = layout(file, true)?;
    parse_resources(&file[l.rsrc_raw..], l.rsrc_rva, true)
}

/// Replace the resources of the executable `file` with `res`.
pub fn write_resources(file: &[u8], res: &Resources) -> Result<Vec<u8>> {
    let l = layout(file, false)?;
    // Every resource must survive the rewrite.
    parse_resources(&file[l.rsrc_raw..], l.rsrc_rva, false)?;
    let section = build_resources(res, l.rsrc_rva);
    let raw_size = align(section.len(), l.file_alignment);
    let mut out = file[..l.rsrc_raw].to_vec();
    out.extend_from_slice(&section);
    out.resize(l.rsrc_raw + raw_size, 0);
    put_u32(&mut out, l.rsrc_header + 8, section.len() as u32);
    put_u32(&mut out, l.rsrc_header + 16, raw_size as u32);
    let image = align(l.rsrc_rva + section.len(), l.section_alignment);
    put_u32(&mut out, l.opt + 56, image as u32);
    let dirs = l.opt + if l.pe32_plus { 112 } else { 96 };
    put_u32(&mut out, dirs + 2 * 8 + 4, section.len() as u32);
    let sum = checksum(&out, l.opt + 64);
    put_u32(&mut out, l.opt + 64, sum);
    Ok(out)
}

/// Replace every icon group (and the icons it uses) with the images of the
/// `.ico` file `ico`, keeping each group's name and language.
pub fn replace_icons(res: &mut Resources, ico: &[u8]) -> Result<()> {
    ensure!(u16_at(ico, 2)? == 1, "not an .ico file");
    let count = u16_at(ico, 4)? as usize;
    let mut images = Vec::new();
    for i in 0..count {
        let e = 6 + 16 * i;
        let size = u32_at(ico, e + 8)? as usize;
        let offset = u32_at(ico, e + 12)? as usize;
        let data = ico.get(offset..offset + size).context("truncated .ico")?;
        images.push((ico[e..e + 8].to_vec(), data.to_vec()));
    }
    let groups = res.remove(&ResId::Id(RT_GROUP_ICON)).unwrap_or_default();
    ensure!(!groups.is_empty(), "the executable has no icon");
    // Drop the icons the old groups used.
    let mut used = Vec::new();
    for leaves in groups.values() {
        for leaf in leaves {
            let n = u16_at(&leaf.data, 4)? as usize;
            for i in 0..n {
                used.push(ResId::Id(u16_at(&leaf.data, 6 + 14 * i + 12)? as u32));
            }
        }
    }
    let icons = res.entry(ResId::Id(RT_ICON)).or_default();
    for id in &used {
        icons.remove(id);
    }
    let mut next_id = icons
        .keys()
        .filter_map(|k| if let ResId::Id(n) = k { Some(*n) } else { None })
        .max()
        .unwrap_or(0)
        + 1;
    let mut new_groups = BTreeMap::new();
    for (name, leaves) in groups {
        let mut new_leaves = Vec::new();
        for leaf in leaves {
            let mut dir = vec![0, 0, 1, 0];
            dir.extend_from_slice(&(images.len() as u16).to_le_bytes());
            for (entry, data) in &images {
                // width, height, colors, reserved, planes, bit count and size
                // as in the .ico; then the icon's resource ID.
                dir.extend_from_slice(&entry[..8]);
                dir.extend_from_slice(&(data.len() as u32).to_le_bytes());
                dir.extend_from_slice(&(next_id as u16).to_le_bytes());
                icons.insert(
                    ResId::Id(next_id),
                    vec![Leaf {
                        lang: leaf.lang,
                        codepage: leaf.codepage,
                        data: data.clone(),
                    }],
                );
                next_id += 1;
            }
            new_leaves.push(Leaf {
                lang: leaf.lang,
                codepage: leaf.codepage,
                data: dir,
            });
        }
        new_groups.insert(name, new_leaves);
    }
    res.insert(ResId::Id(RT_GROUP_ICON), new_groups);
    Ok(())
}

/// A node of a VS_VERSIONINFO tree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VersionNode {
    key: String,
    /// 1 for text values, 0 for binary ones.
    kind: u16,
    value: Vec<u8>,
    /// For text values: the length in UTF-16 units (Windows counts the
    /// terminating NUL); for binary ones, in bytes.
    value_len: u16,
    children: Vec<VersionNode>,
}

fn parse_version_node(d: &[u8], start: usize) -> Result<(VersionNode, usize)> {
    let len = u16_at(d, start)? as usize;
    let value_len = u16_at(d, start + 2)?;
    let kind = u16_at(d, start + 4)?;
    let end = start + len;
    ensure!(end <= d.len() && len >= 6, "malformed version resource");
    let mut o = start + 6;
    let mut key = Vec::new();
    loop {
        let u = u16_at(d, o)?;
        o += 2;
        if u == 0 {
            break;
        }
        key.push(u);
    }
    o = align(o, 4);
    let value_bytes = if kind == 1 {
        2 * value_len as usize
    } else {
        value_len as usize
    };
    let value = d.get(o..(o + value_bytes).min(end)).unwrap_or(&[]).to_vec();
    o = align(o + value_bytes, 4);
    let mut children = Vec::new();
    while o + 6 <= end {
        let (child, next) = parse_version_node(d, o)?;
        children.push(child);
        o = align(next, 4);
    }
    Ok((
        VersionNode {
            key: String::from_utf16(&key)?,
            kind,
            value,
            value_len,
            children,
        },
        end,
    ))
}

fn write_version_node(node: &VersionNode, out: &mut Vec<u8>) {
    let start = out.len();
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&node.value_len.to_le_bytes());
    out.extend_from_slice(&node.kind.to_le_bytes());
    for u in node.key.encode_utf16().chain([0]) {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.resize(align(out.len(), 4), 0);
    out.extend_from_slice(&node.value);
    for child in &node.children {
        out.resize(align(out.len(), 4), 0);
        write_version_node(child, out);
    }
    let len = (out.len() - start) as u16;
    out[start..start + 2].copy_from_slice(&len.to_le_bytes());
}

/// Set the strings (CompanyName, ProductName, ...) of every version
/// resource; strings the resource does not have are added.
pub fn set_version_strings(res: &mut Resources, strings: &[(&str, &str)]) -> Result<()> {
    let versions = res
        .get_mut(&ResId::Id(RT_VERSION))
        .context("the executable has no version resource")?;
    for leaf in versions.values_mut().flatten() {
        let (mut root, _) = parse_version_node(&leaf.data, 0)?;
        let file_info = root
            .children
            .iter_mut()
            .find(|c| c.key == "StringFileInfo")
            .context("version resource has no StringFileInfo")?;
        for table in &mut file_info.children {
            for (key, value) in strings {
                let units: Vec<u16> = value.encode_utf16().chain([0]).collect();
                let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
                let node = VersionNode {
                    key: key.to_string(),
                    kind: 1,
                    value: bytes,
                    value_len: units.len() as u16,
                    children: vec![],
                };
                match table.children.iter_mut().find(|c| c.key == *key) {
                    Some(existing) => *existing = node,
                    None => table.children.push(node),
                }
            }
        }
        let mut data = Vec::new();
        write_version_node(&root, &mut data);
        leaf.data = data;
    }
    Ok(())
}

/// The image sizes of every icon group, in order.
pub fn icon_group_sizes(res: &Resources) -> Result<Vec<u32>> {
    let mut sizes = Vec::new();
    for leaf in res
        .get(&ResId::Id(RT_GROUP_ICON))
        .into_iter()
        .flat_map(|g| g.values().flatten())
    {
        for i in 0..u16_at(&leaf.data, 4)? as usize {
            let w = leaf.data[6 + 14 * i] as u32;
            sizes.push(if w == 0 { 256 } else { w });
        }
    }
    Ok(sizes)
}

/// The strings of the first version resource.
pub fn version_strings(res: &Resources) -> Result<Vec<(String, String)>> {
    let leaf = res
        .get(&ResId::Id(RT_VERSION))
        .and_then(|v| v.values().flatten().next())
        .context("no version resource")?;
    let (root, _) = parse_version_node(&leaf.data, 0)?;
    let mut out = Vec::new();
    for info in root.children.iter().filter(|c| c.key == "StringFileInfo") {
        for table in &info.children {
            for s in &table.children {
                let units: Vec<u16> = s
                    .value
                    .chunks(2)
                    .map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                    .take_while(|&u| u != 0)
                    .collect();
                out.push((s.key.clone(), String::from_utf16_lossy(&units)));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE32 executable whose only section is `.rsrc`.
    fn tiny_pe(res: &Resources) -> Vec<u8> {
        let (file_align, section_align, rva) = (0x200usize, 0x1000usize, 0x1000usize);
        let mut f = vec![0u8; 0x200];
        f[0..2].copy_from_slice(b"MZ");
        put_u32(&mut f, 0x3c, 0x40);
        f[0x40..0x44].copy_from_slice(b"PE\0\0");
        put_u16(&mut f, 0x44, 0x14c);
        put_u16(&mut f, 0x46, 1); // sections
        put_u16(&mut f, 0x54, 224); // optional header size
        let opt = 0x58;
        put_u16(&mut f, opt, 0x10b);
        put_u32(&mut f, opt + 32, section_align as u32);
        put_u32(&mut f, opt + 36, file_align as u32);
        put_u32(&mut f, opt + 92, 16); // data directories
        let section = build_resources(res, rva);
        put_u32(&mut f, opt + 96 + 16, rva as u32);
        put_u32(&mut f, opt + 96 + 20, section.len() as u32);
        let h = opt + 224;
        f[h..h + 5].copy_from_slice(b".rsrc");
        put_u32(&mut f, h + 8, section.len() as u32);
        put_u32(&mut f, h + 12, rva as u32);
        put_u32(&mut f, h + 16, align(section.len(), file_align) as u32);
        put_u32(&mut f, h + 20, 0x200);
        f.extend_from_slice(&section);
        f.resize(0x200 + align(section.len(), file_align), 0);
        f
    }

    fn version_resource() -> Vec<u8> {
        let text = |key: &str, value: &str| {
            let units: Vec<u16> = value.encode_utf16().chain([0]).collect();
            VersionNode {
                key: key.into(),
                kind: 1,
                value: units.iter().flat_map(|u| u.to_le_bytes()).collect(),
                value_len: units.len() as u16,
                children: vec![],
            }
        };
        let root = VersionNode {
            key: "VS_VERSION_INFO".into(),
            kind: 0,
            value: vec![0xbd, 0x04, 0xef, 0xfe]
                .into_iter()
                .chain([0; 48])
                .collect(),
            value_len: 52,
            children: vec![
                VersionNode {
                    key: "StringFileInfo".into(),
                    kind: 1,
                    value: vec![],
                    value_len: 0,
                    children: vec![VersionNode {
                        key: "040904b0".into(),
                        kind: 1,
                        value: vec![],
                        value_len: 0,
                        children: vec![
                            text("CompanyName", "Mozilla"),
                            text("ProductName", "Firefox"),
                        ],
                    }],
                },
                VersionNode {
                    key: "VarFileInfo".into(),
                    kind: 1,
                    value: vec![],
                    value_len: 0,
                    children: vec![VersionNode {
                        key: "Translation".into(),
                        kind: 0,
                        value: vec![0x09, 0x04, 0xb0, 0x04],
                        value_len: 4,
                        children: vec![],
                    }],
                },
            ],
        };
        let mut out = Vec::new();
        write_version_node(&root, &mut out);
        out
    }

    fn sample_resources() -> Resources {
        let leaf = |data: Vec<u8>| {
            vec![Leaf {
                lang: 0x409,
                codepage: 0,
                data,
            }]
        };
        let mut group = vec![0, 0, 1, 0, 1, 0];
        group.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0]);
        group.extend_from_slice(&3u32.to_le_bytes());
        group.extend_from_slice(&1u16.to_le_bytes());
        let mut res = Resources::new();
        res.entry(ResId::Id(RT_ICON))
            .or_default()
            .insert(ResId::Id(1), leaf(vec![1, 2, 3]));
        res.entry(ResId::Id(RT_GROUP_ICON))
            .or_default()
            .insert(ResId::Id(1), leaf(group));
        res.entry(ResId::Id(RT_VERSION))
            .or_default()
            .insert(ResId::Id(1), leaf(version_resource()));
        res.entry(ResId::Name("CUSTOM".into()))
            .or_default()
            .insert(ResId::Name("THING".into()), leaf(b"hello".to_vec()));
        res
    }

    #[test]
    fn resources_round_trip() {
        let res = sample_resources();
        let pe = tiny_pe(&res);
        assert_eq!(read_resources(&pe).unwrap(), res);
        let rewritten = write_resources(&pe, &res).unwrap();
        assert_eq!(read_resources(&rewritten).unwrap(), res);
    }

    #[test]
    fn icons_and_version_strings_are_replaced() {
        let pe = tiny_pe(&sample_resources());
        let images: Vec<image::RgbaImage> = [16u32, 32, 256]
            .iter()
            .map(|&s| image::RgbaImage::from_pixel(s, s, image::Rgba([1, 2, 3, 255])))
            .collect();
        let ico = crate::windows::ico(&images).unwrap();
        let mut res = read_resources(&pe).unwrap();
        replace_icons(&mut res, &ico).unwrap();
        set_version_strings(
            &mut res,
            &[
                ("CompanyName", "Hallow"),
                ("FileDescription", "Hallow Setup"),
            ],
        )
        .unwrap();
        let out = write_resources(&pe, &res).unwrap();
        let back = read_resources(&out).unwrap();

        let icons = &back[&ResId::Id(RT_ICON)];
        assert_eq!(icons.len(), 3, "old icon dropped, three new ones");
        let group = &back[&ResId::Id(RT_GROUP_ICON)][&ResId::Id(1)][0].data;
        assert_eq!(u16_at(group, 4).unwrap(), 3);
        let sizes: Vec<u8> = (0..3).map(|i| group[6 + 14 * i]).collect();
        assert_eq!(sizes, [16, 32, 0]);
        for i in 0..3 {
            let id = u16_at(group, 6 + 14 * i + 12).unwrap() as u32;
            let size = u32_at(group, 6 + 14 * i + 8).unwrap() as usize;
            assert_eq!(icons[&ResId::Id(id)][0].data.len(), size);
        }
        let strings = version_strings(&back).unwrap();
        assert!(strings.contains(&("CompanyName".into(), "Hallow".into())));
        assert!(strings.contains(&("ProductName".into(), "Firefox".into())));
        assert!(strings.contains(&("FileDescription".into(), "Hallow Setup".into())));
        // Other resources are untouched; the image size covers the section.
        assert_eq!(
            back[&ResId::Name("CUSTOM".into())],
            sample_resources()[&ResId::Name("CUSTOM".into())]
        );
        let l = layout(&out, false).unwrap();
        let image_size = u32_at(&out, l.opt + 56).unwrap() as usize;
        let section_size = u32_at(&out, l.rsrc_header + 8).unwrap() as usize;
        assert!(image_size >= l.rsrc_rva + section_size);
        assert_eq!(out.len() % 0x200, 0);
    }

    #[test]
    fn version_tree_round_trips() {
        let data = version_resource();
        let (root, end) = parse_version_node(&data, 0).unwrap();
        assert_eq!(end, data.len());
        let mut again = Vec::new();
        write_version_node(&root, &mut again);
        assert_eq!(again, data);
    }

    #[test]
    fn rewrites_only_files_ending_with_their_resources() {
        let mut pe = tiny_pe(&sample_resources());
        pe.extend_from_slice(b"appended archive");
        // Readable (an installer: stub + archive), but not rewritable.
        assert_eq!(read_resources(&pe).unwrap(), sample_resources());
        assert!(write_resources(&pe, &sample_resources()).is_err());
    }

    #[test]
    fn icon_sizes_of_groups() {
        let res = sample_resources();
        assert_eq!(icon_group_sizes(&res).unwrap(), vec![16]);
    }
}
