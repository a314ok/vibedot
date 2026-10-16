//! Asset access: a project directory during development, or a single
//! `.vpk` pack file in exported games (Godot's `.pck`).
//!
//! # VPK format (little endian)
//!
//! ```text
//! offset  size  field
//! 0       4     magic "VDPK"
//! 4       4     format version (1)
//! 8       4     file count N
//! 12      8     total data size
//! 20      ...   N entries: u16 path_len, path (UTF-8), u64 offset, u64 size, u32 crc32
//! ...     ...   file data (offsets are relative to the start of this region)
//! ```
//!
//! Every file is checked against its CRC-32 when read, so a corrupted
//! download fails loudly instead of producing a broken game.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const PACK_MAGIC: &[u8; 4] = b"VDPK";
pub const PACK_VERSION: u32 = 1;

/// Read-only file system abstraction used by the runtime.
pub trait AssetSource: Send + Sync {
    /// Reads a file by project-relative path (`"scenes/main.vscn"`).
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    fn read_string(&self, path: &str) -> Option<String> {
        String::from_utf8(self.read(path)?).ok()
    }
    /// All file paths (used by the editor's file browser and the packer).
    fn list(&self) -> Vec<String>;
}

/// Normalizes `res://a/b`, `./a/b` and `a\\b` to `a/b`.
pub fn normalize_path(path: &str) -> String {
    let p = path.trim().trim_start_matches("res://").replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Project directory on disk.
#[derive(Clone, Debug)]
pub struct DirAssets {
    pub root: PathBuf,
}

impl DirAssets {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn full_path(&self, path: &str) -> PathBuf {
        self.root.join(normalize_path(path))
    }
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "target" || name == "export" {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&e.path(), &rel, out);
        } else if ft.is_file() {
            out.push(rel);
        }
    }
}

impl AssetSource for DirAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(self.full_path(path)).ok()
    }

    fn list(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.root, "", &mut out);
        out
    }
}

/// In-memory assets (tests, embedded demos).
#[derive(Clone, Debug, Default)]
pub struct MemAssets {
    pub files: HashMap<String, Vec<u8>>,
}

impl MemAssets {
    pub fn insert(&mut self, path: &str, data: impl Into<Vec<u8>>) {
        self.files.insert(normalize_path(path), data.into());
    }
}

impl AssetSource for MemAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(&normalize_path(path)).cloned()
    }
    fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.files.keys().cloned().collect();
        v.sort();
        v
    }
}

// ---------------------------------------------------------------- CRC-32

/// CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320), table generated at
/// compile time.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------- pack

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Vibedot pack (bad magic)")]
    Magic,
    #[error("unsupported pack version {0}")]
    Version(u32),
    #[error("pack is truncated or corrupted")]
    Truncated,
    #[error("checksum mismatch for '{0}'")]
    Checksum(String),
}

#[derive(Clone, Debug)]
struct PackEntry {
    offset: u64,
    size: u64,
    crc: u32,
}

/// A loaded `.vpk` file (kept in memory; games are small).
#[derive(Clone, Debug)]
pub struct PackAssets {
    data: Arc<Vec<u8>>,
    data_start: usize,
    entries: HashMap<String, PackEntry>,
}

/// Tiny cursor over a byte slice with bounds-checked little-endian reads.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PackError> {
        let end = self.pos.checked_add(n).ok_or(PackError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(PackError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, PackError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2 bytes")))
    }
    fn u32(&mut self) -> Result<u32, PackError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn u64(&mut self) -> Result<u64, PackError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
}

impl PackAssets {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, PackError> {
        let mut r = Reader { buf: &bytes, pos: 0 };
        if r.take(4)? != PACK_MAGIC {
            return Err(PackError::Magic);
        }
        let version = r.u32()?;
        if version != PACK_VERSION {
            return Err(PackError::Version(version));
        }
        let count = r.u32()? as usize;
        let total = r.u64()?;
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            let len = r.u16()? as usize;
            let path = String::from_utf8(r.take(len)?.to_vec()).map_err(|_| PackError::Truncated)?;
            let offset = r.u64()?;
            let size = r.u64()?;
            let crc = r.u32()?;
            if offset.checked_add(size).is_none_or(|end| end > total) {
                return Err(PackError::Truncated);
            }
            entries.insert(path, PackEntry { offset, size, crc });
        }
        let data_start = r.pos;
        if (bytes.len() - data_start) as u64 != total {
            return Err(PackError::Truncated);
        }
        Ok(Self { data: Arc::new(bytes), data_start, entries })
    }

    pub fn open(path: &Path) -> Result<Self, PackError> {
        Self::from_bytes(fs::read(path)?)
    }

    /// Verifies every file's checksum.
    pub fn verify(&self) -> Result<(), PackError> {
        for (path, e) in &self.entries {
            let start = self.data_start + e.offset as usize;
            if crc32(&self.data[start..start + e.size as usize]) != e.crc {
                return Err(PackError::Checksum(path.clone()));
            }
        }
        Ok(())
    }
}

impl AssetSource for PackAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&normalize_path(path))?;
        let start = self.data_start + e.offset as usize;
        let bytes = &self.data[start..start + e.size as usize];
        if crc32(bytes) != e.crc {
            log::error!("pack checksum mismatch for '{path}'");
            return None;
        }
        Some(bytes.to_vec())
    }

    fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }
}

/// Builds a pack from `(path, bytes)` pairs.
pub fn write_pack(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(PACK_MAGIC);
    header.extend_from_slice(&PACK_VERSION.to_le_bytes());
    header.extend_from_slice(&(files.len() as u32).to_le_bytes());
    let total: u64 = files.iter().map(|(_, d)| d.len() as u64).sum();
    header.extend_from_slice(&total.to_le_bytes());
    let mut offset = 0u64;
    for (path, data) in files {
        let p = normalize_path(path);
        header.extend_from_slice(&(p.len() as u16).to_le_bytes());
        header.extend_from_slice(p.as_bytes());
        header.extend_from_slice(&offset.to_le_bytes());
        header.extend_from_slice(&(data.len() as u64).to_le_bytes());
        header.extend_from_slice(&crc32(data).to_le_bytes());
        offset += data.len() as u64;
    }
    for (_, data) in files {
        header.extend_from_slice(data);
    }
    header
}

/// Packs every file of an asset source (skipping editor-only files).
pub fn pack_source(src: &dyn AssetSource) -> Vec<u8> {
    let files: Vec<(String, Vec<u8>)> = src
        .list()
        .into_iter()
        .filter(|p| !p.ends_with(".tmp") && !p.starts_with("export/"))
        .filter_map(|p| src.read(&p).map(|d| (p, d)))
        .collect();
    write_pack(&files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn pack_roundtrip_and_corruption() {
        let files =
            vec![("scenes/main.vscn".to_owned(), b"{\"scene\":1}".to_vec()), ("res://icon.png".to_owned(), vec![0u8, 1, 2, 3, 255])];
        let bytes = write_pack(&files);
        let pack = PackAssets::from_bytes(bytes.clone()).unwrap();
        pack.verify().unwrap();
        assert_eq!(pack.read("scenes/main.vscn").unwrap(), b"{\"scene\":1}");
        assert_eq!(pack.read("./icon.png").unwrap(), vec![0, 1, 2, 3, 255]);
        assert_eq!(pack.list(), vec!["icon.png", "scenes/main.vscn"]);
        // Flip one data byte: read must fail the checksum.
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        let pack = PackAssets::from_bytes(bad).unwrap();
        assert!(pack.read("icon.png").is_none());
        assert!(matches!(pack.verify(), Err(PackError::Checksum(_))));
        // Truncation and bad magic are detected.
        assert!(matches!(PackAssets::from_bytes(bytes[..bytes.len() - 2].to_vec()), Err(PackError::Truncated)));
        assert!(matches!(PackAssets::from_bytes(b"NOPE".to_vec()), Err(PackError::Magic)));
    }

    #[test]
    fn path_normalization() {
        assert_eq!(normalize_path("res://a/./b/../c.png"), "a/c.png");
        assert_eq!(normalize_path("a\\b"), "a/b");
    }
}
