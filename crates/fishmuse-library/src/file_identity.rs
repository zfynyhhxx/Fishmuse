use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const SAMPLE_SIZE: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickFileIdentity {
    pub size: u64,
    pub modified_ns: u128,
    pub quick_fingerprint: String,
}

impl QuickFileIdentity {
    pub fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        let modified_ns = modified_ns(&metadata);
        let mut hasher = blake3::Hasher::new();
        hasher.update(&size.to_le_bytes());
        hasher.update(&modified_ns.to_le_bytes());
        match stable_file_id(path) {
            Some(stable_id) => {
                hasher.update(b"stable-id");
                hasher.update(&stable_id);
            }
            None => {
                hasher.update(b"path-fallback");
                hasher.update(&normalize_path_bytes(path));
            }
        }
        hash_edge_samples(path, size, &mut hasher)?;
        Ok(Self {
            size,
            modified_ns,
            quick_fingerprint: hasher.finalize().to_hex().to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub size: u64,
    pub modified_ns: u128,
    pub quick_fingerprint: String,
    pub content_fingerprint: String,
}

impl FileIdentity {
    pub fn read(path: &Path) -> io::Result<Self> {
        let quick = QuickFileIdentity::read(path)?;
        let mut file = File::open(path)?;
        let mut hasher = blake3::Hasher::new();
        io::copy(&mut file, &mut hasher)?;
        Ok(Self {
            size: quick.size,
            modified_ns: quick.modified_ns,
            quick_fingerprint: quick.quick_fingerprint,
            content_fingerprint: hasher.finalize().to_hex().to_string(),
        })
    }
}

fn modified_ns(metadata: &Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

fn hash_edge_samples(path: &Path, size: u64, hasher: &mut blake3::Hasher) -> io::Result<()> {
    let mut file = File::open(path)?;
    let first_length = usize::try_from(size.min(SAMPLE_SIZE as u64)).unwrap_or(SAMPLE_SIZE);
    let mut first = vec![0; first_length];
    file.read_exact(&mut first)?;
    hasher.update(&first);

    if size > SAMPLE_SIZE as u64 {
        let tail_length = usize::try_from(size.min(SAMPLE_SIZE as u64)).unwrap_or(SAMPLE_SIZE);
        file.seek(SeekFrom::End(-(tail_length as i64)))?;
        let mut tail = vec![0; tail_length];
        file.read_exact(&mut tail)?;
        hasher.update(&tail);
    }
    Ok(())
}

fn stable_file_id(path: &Path) -> Option<Vec<u8>> {
    let id = file_id::get_file_id(path).ok()?;
    let mut bytes = Vec::with_capacity(25);
    match id {
        file_id::FileId::Inode {
            device_id,
            inode_number,
        } => {
            bytes.push(0);
            bytes.extend_from_slice(&device_id.to_le_bytes());
            bytes.extend_from_slice(&inode_number.to_le_bytes());
        }
        file_id::FileId::LowRes {
            volume_serial_number,
            file_index,
        } => {
            bytes.push(1);
            bytes.extend_from_slice(&volume_serial_number.to_le_bytes());
            bytes.extend_from_slice(&file_index.to_le_bytes());
        }
        file_id::FileId::HighRes {
            volume_serial_number,
            file_id,
        } => {
            bytes.push(2);
            bytes.extend_from_slice(&volume_serial_number.to_le_bytes());
            bytes.extend_from_slice(&file_id.to_le_bytes());
        }
    }
    Some(bytes)
}

#[cfg(windows)]
#[must_use]
pub fn normalize_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    const LONG_UNC: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    const LONG_PATH: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if starts_with_ascii_case_insensitive(&units, LONG_UNC) {
        units.splice(..LONG_UNC.len(), [b'\\' as u16, b'\\' as u16]);
    } else if starts_with_ascii_case_insensitive(&units, LONG_PATH) {
        units.drain(..LONG_PATH.len());
    }
    for unit in &mut units {
        if *unit == b'/' as u16 {
            *unit = b'\\' as u16;
        }
    }
    if let Ok(path_text) = String::from_utf16(&units) {
        units = path_text.to_lowercase().encode_utf16().collect();
    } else {
        for unit in &mut units {
            *unit = ascii_lower_u16(*unit);
        }
    }
    while units.len() > 3 && units.last() == Some(&(b'\\' as u16)) {
        units.pop();
    }
    units
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
}

#[cfg(windows)]
fn starts_with_ascii_case_insensitive(value: &[u16], prefix: &[u16]) -> bool {
    value.len() >= prefix.len()
        && value
            .iter()
            .zip(prefix)
            .all(|(left, right)| ascii_lower_u16(*left) == ascii_lower_u16(*right))
}

#[cfg(windows)]
fn ascii_lower_u16(value: u16) -> u16 {
    if value >= b'A' as u16 && value <= b'Z' as u16 {
        value + 32
    } else {
        value
    }
}

#[cfg(unix)]
#[must_use]
pub fn normalize_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(any(unix, windows)))]
#[must_use]
pub fn normalize_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

pub fn discover_files(roots: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut visited = HashSet::new();
    for root in roots {
        let root = fs::canonicalize(root)?;
        let mut pending = VecDeque::from([root]);
        while let Some(directory) = pending.pop_front() {
            let canonical = fs::canonicalize(&directory)?;
            let normalized = normalize_path_bytes(&canonical);
            if !visited.insert(normalized) {
                continue;
            }
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if is_link_or_reparse_point(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    pending.push_back(path);
                } else if metadata.is_file() {
                    files.push(path);
                }
            }
        }
    }
    files.sort_by_cached_key(|path| normalize_path_bytes(path));
    Ok(files)
}

#[cfg(windows)]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}
