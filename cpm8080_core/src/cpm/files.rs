use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const RECORD: usize = 128;

/// A CP/M file name: 8 name bytes and 3 type bytes, space padded.
pub type Name = [u8; 11];

/// Converts a host file name to a CP/M name, or None when it doesn't fit 8.3.
pub fn host_to_cpm(host: &str) -> Option<Name> {
    let (base, ext) = match host.rfind('.') {
        Some(i) => (&host[..i], &host[i + 1..]),
        None => (host, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return None;
    }
    let mut name = [b' '; 11];
    for (i, c) in base.bytes().enumerate() {
        name[i] = cpm_char(c)?;
    }
    for (i, c) in ext.bytes().enumerate() {
        name[8 + i] = cpm_char(c)?;
    }
    Some(name)
}

fn cpm_char(c: u8) -> Option<u8> {
    if c.is_ascii_graphic() && !b".*?:;,=<>[]|".contains(&c) {
        Some(c.to_ascii_uppercase())
    } else {
        None
    }
}

/// The host file name used when creating a file: NAME.EXT, or NAME without a type.
pub fn cpm_to_host(name: &Name) -> String {
    let base = String::from_utf8_lossy(&name[..8]).trim_end().to_string();
    let ext = String::from_utf8_lossy(&name[8..]).trim_end().to_string();
    if ext.is_empty() {
        base
    } else {
        format!("{}.{}", base, ext)
    }
}

/// Matches a name against a pattern where '?' matches any character.
pub fn matches(pattern: &Name, name: &Name) -> bool {
    pattern
        .iter()
        .zip(name.iter())
        .all(|(p, n)| *p == b'?' || *p == *n)
}

/// All files in `root` that match `pattern`, sorted by name.
pub fn search(root: &Path, pattern: &Name) -> Vec<(Name, PathBuf)> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            let host = entry.file_name();
            if let (true, Some(host)) = (is_file, host.to_str()) {
                if let Some(name) = host_to_cpm(host) {
                    if matches(pattern, &name) {
                        found.push((name, entry.path()));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

pub fn find(root: &Path, pattern: &Name) -> Option<PathBuf> {
    search(root, pattern).into_iter().next().map(|(_, path)| path)
}

/// Number of 128-byte records in the file, counting a partial last record.
pub fn records(path: &Path) -> u32 {
    let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    len.div_ceil(RECORD as u64) as u32
}

/// Reads record `rec` into `buf`, padding a short last record with ^Z.
/// Returns Ok(false) when the record is past the end of the file.
pub fn read_record(path: &Path, rec: u32, buf: &mut [u8; RECORD]) -> io::Result<bool> {
    let mut file = File::open(path)?;
    let pos = rec as u64 * RECORD as u64;
    if pos >= file.metadata()?.len() {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(pos))?;
    let mut filled = 0;
    while filled < RECORD {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf[filled..].fill(0x1A);
    Ok(true)
}

/// Writes record `rec`, extending the file with zeros if it is past the end.
pub fn write_record(path: &Path, rec: u32, buf: &[u8; RECORD]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(rec as u64 * RECORD as u64))?;
    file.write_all(buf)
}
