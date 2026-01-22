use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use tokio::task;

use crate::error::AppError;

const SECTOR_SIZE: usize = 2048;
const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks

/// A directory entry from ISO9660
#[derive(Debug, Clone)]
pub struct IsoEntry {
    pub name: String,
    pub is_dir: bool,
    pub lba: u32,
    pub size: u32,
}

/// ISO9660 reader that properly handles multi-sector directories
pub struct Iso9660Reader {
    file: File,
    root_lba: u32,
    root_size: u32,
}

impl Iso9660Reader {
    pub fn new(mut file: File) -> Result<Self, AppError> {
        // Read Primary Volume Descriptor at sector 16
        let mut sector = [0u8; SECTOR_SIZE];
        file.seek(SeekFrom::Start(16 * SECTOR_SIZE as u64))?;
        file.read_exact(&mut sector)?;

        // Verify CD001 signature at offset 1
        if &sector[1..6] != b"CD001" {
            return Err(AppError::Iso("Invalid ISO9660 signature".to_string()));
        }

        // Root directory record is at offset 156 in the PVD
        let root_record = &sector[156..190];
        let root_lba = u32::from_le_bytes([root_record[2], root_record[3], root_record[4], root_record[5]]);
        let root_size = u32::from_le_bytes([root_record[10], root_record[11], root_record[12], root_record[13]]);

        Ok(Self {
            file,
            root_lba,
            root_size,
        })
    }

    /// Read directory entries at given LBA
    pub fn read_directory(&mut self, lba: u32, dir_size: u32) -> Result<Vec<IsoEntry>, AppError> {
        let mut entries = Vec::new();
        let dir_size = dir_size as usize;
        let mut buffer = vec![0u8; dir_size];

        self.file.seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        self.file.read_exact(&mut buffer)?;

        let mut offset = 0usize;
        while offset < dir_size {
            // Bounds check for record length byte
            if offset >= buffer.len() {
                break;
            }

            let record_len = buffer[offset] as usize;

            // Zero length means padding to sector boundary - skip to next sector
            if record_len == 0 {
                let next_sector = ((offset / SECTOR_SIZE) + 1) * SECTOR_SIZE;
                if next_sector >= dir_size {
                    break;
                }
                offset = next_sector;
                continue;
            }

            // Bounds check: ensure we have enough data for the minimum record
            if offset + 33 > buffer.len() {
                break;
            }

            // Parse directory record
            let flags = buffer[offset + 25];
            let is_dir = (flags & 0x02) != 0;

            let entry_lba = u32::from_le_bytes([
                buffer[offset + 2],
                buffer[offset + 3],
                buffer[offset + 4],
                buffer[offset + 5],
            ]);
            let entry_size = u32::from_le_bytes([
                buffer[offset + 10],
                buffer[offset + 11],
                buffer[offset + 12],
                buffer[offset + 13],
            ]);

            let name_len = buffer[offset + 32] as usize;

            // Bounds check for name
            let name_start = offset + 33;
            let name_end = name_start + name_len;
            if name_end > buffer.len() {
                break;
            }

            let name_bytes = &buffer[name_start..name_end];

            // Parse name
            let name = if name_len == 1 && name_bytes[0] == 0 {
                ".".to_string()
            } else if name_len == 1 && name_bytes[0] == 1 {
                "..".to_string()
            } else {
                // Remove version number (;1) if present
                let raw_name = String::from_utf8_lossy(name_bytes);
                let clean_name = raw_name.split(';').next().unwrap_or(&raw_name);
                // Remove trailing dot if present
                clean_name.trim_end_matches('.').to_string()
            };

            entries.push(IsoEntry {
                name,
                is_dir,
                lba: entry_lba,
                size: entry_size,
            });

            offset += record_len;
        }

        Ok(entries)
    }

    /// Read root directory entries
    pub fn read_root(&mut self) -> Result<Vec<IsoEntry>, AppError> {
        self.read_directory(self.root_lba, self.root_size)
    }

    /// Read a chunk of file data at given offset
    pub fn read_file_chunk(&mut self, lba: u32, file_offset: u64, chunk_size: usize) -> Result<Vec<u8>, AppError> {
        let byte_offset = lba as u64 * SECTOR_SIZE as u64 + file_offset;
        let mut data = vec![0u8; chunk_size];
        self.file.seek(SeekFrom::Start(byte_offset))?;
        self.file.read_exact(&mut data)?;
        Ok(data)
    }

    /// Read entire file contents at given LBA and size (for small files)
    pub fn read_file_data(&mut self, lba: u32, size: u32) -> Result<Vec<u8>, AppError> {
        let mut data = vec![0u8; size as usize];
        self.file.seek(SeekFrom::Start(lba as u64 * SECTOR_SIZE as u64))?;
        self.file.read_exact(&mut data)?;
        Ok(data)
    }
}

/// File location info for streaming
#[derive(Clone)]
pub struct IsoFileLocation {
    pub iso_path: Arc<Path>,
    pub lba: u32,
    pub size: u64,
}

/// Find a file by path in the ISO and return its location
pub fn find_file_location(iso_path: &Path, file_path: &str) -> Result<IsoFileLocation, AppError> {
    let file = File::open(iso_path)?;
    let mut iso = Iso9660Reader::new(file)?;
    let entry = find_file_entry(&mut iso, file_path)?;

    if entry.is_dir {
        return Err(AppError::BadRequest(format!("'{}' is a directory", file_path)));
    }

    Ok(IsoFileLocation {
        iso_path: Arc::from(iso_path),
        lba: entry.lba,
        size: entry.size as u64,
    })
}

/// Find a file by path in the ISO
fn find_file_entry(iso: &mut Iso9660Reader, file_path: &str) -> Result<IsoEntry, AppError> {
    let normalized_path = file_path.trim_start_matches('/');
    let components: Vec<&str> = normalized_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if components.is_empty() {
        return Err(AppError::BadRequest("Empty file path".to_string()));
    }

    let mut current_entries = iso.read_root()?;

    for (depth, target) in components.iter().enumerate() {
        let is_last = depth == components.len() - 1;

        let entry = current_entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(target) && e.name != "." && e.name != "..")
            .ok_or_else(|| {
                AppError::NotFound(format!("File not found in ISO: {}", file_path))
            })?;

        if is_last {
            return Ok(entry.clone());
        }

        if !entry.is_dir {
            return Err(AppError::BadRequest(format!(
                "'{}' is a file, expected directory",
                components[..=depth].join("/")
            )));
        }

        current_entries = iso.read_directory(entry.lba, entry.size)?;
    }

    Err(AppError::NotFound(format!("File not found in ISO: {}", file_path)))
}

/// Get file size from ISO without reading content
pub fn get_file_size(iso_path: &Path, file_path: &str) -> Result<u64, AppError> {
    let file = File::open(iso_path)?;
    let mut iso = Iso9660Reader::new(file)?;
    let entry = find_file_entry(&mut iso, file_path)?;
    Ok(entry.size as u64)
}

/// Stream file from ISO in 1MB chunks
/// Uses smart allocation: up to 2MB initially, then 1MB or remaining size
pub fn stream_file(iso_path: &Path, file_path: &str) -> Result<(u64, Body), AppError> {
    let location = find_file_location(iso_path, file_path)?;
    let size = location.size;

    // For small files (<=2MB), read entirely - more efficient
    if size <= 2 * CHUNK_SIZE as u64 {
        let file = File::open(iso_path)?;
        let mut iso = Iso9660Reader::new(file)?;
        let data = iso.read_file_chunk(location.lba, 0, size as usize)?;
        return Ok((size, Body::from(data)));
    }

    // For larger files, stream in 1MB chunks
    let iso_path = location.iso_path.clone();
    let lba = location.lba;

    // Generate chunk offsets
    let mut offsets = Vec::new();
    let mut offset = 0u64;
    while offset < size {
        let chunk_size = std::cmp::min(CHUNK_SIZE as u64, size - offset);
        offsets.push((offset, chunk_size as usize));
        offset += chunk_size as u64;
    }

    let stream = stream::iter(offsets.into_iter().map(move |(offset, chunk_size)| {
        let iso_path = iso_path.clone();
        async move {
            // Use spawn_blocking for synchronous file I/O
            let result = task::spawn_blocking(move || {
                let file = File::open(iso_path.as_ref())?;
                let mut iso = Iso9660Reader::new(file)?;
                iso.read_file_chunk(lba, offset, chunk_size)
            })
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            result.map(Bytes::from).map_err(|e: AppError| {
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })
        }
    }))
    .buffered(2); // Buffer up to 2 chunks for better throughput

    Ok((size, Body::from_stream(stream)))
}

/// Read entire file from ISO into memory (for small files like templates)
pub fn read_file(iso_path: &Path, file_path: &str) -> Result<Vec<u8>, AppError> {
    let file = File::open(iso_path)?;
    let mut iso = Iso9660Reader::new(file)?;
    let entry = find_file_entry(&mut iso, file_path)?;

    if entry.is_dir {
        return Err(AppError::BadRequest(format!("'{}' is a directory", file_path)));
    }

    iso.read_file_data(entry.lba, entry.size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_reader_with_100_files() {
        let iso_path = Path::new("/tmp/serabut_iso_test/test.iso");
        if !iso_path.exists() {
            println!("Skipping test - ISO not found. Create it first with:");
            println!("  mkdir -p /tmp/serabut_iso_test/content/a/b/c/e/f");
            println!("  for i in $(seq -w 1 100); do echo \"content $i\" > /tmp/serabut_iso_test/content/a/b/c/e/f/$i.txt; done");
            println!("  genisoimage -o /tmp/serabut_iso_test/test.iso -R -J /tmp/serabut_iso_test/content");
            return;
        }

        let file = File::open(iso_path).expect("Failed to open ISO");
        let mut iso = Iso9660Reader::new(file).expect("Failed to parse ISO");

        // Recursively collect all files
        fn collect_files(iso: &mut Iso9660Reader, lba: u32, size: u32, prefix: &str) -> Vec<String> {
            let mut files = Vec::new();
            let entries = iso.read_directory(lba, size).unwrap();

            for entry in entries {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }

                let full_path = if prefix.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{}/{}", prefix, entry.name)
                };

                if entry.is_dir {
                    let sub_files = collect_files(iso, entry.lba, entry.size, &full_path);
                    files.extend(sub_files);
                } else {
                    files.push(full_path);
                }
            }
            files
        }

        let root_lba = iso.root_lba;
        let root_size = iso.root_size;
        let files = collect_files(&mut iso, root_lba, root_size, "");

        println!("\nFound {} files:", files.len());
        for f in &files {
            println!("  {}", f);
        }

        let txt_files: Vec<_> = files.iter().filter(|f| f.ends_with(".txt") || f.ends_with(".TXT")).collect();
        println!("\nFound {} .txt files", txt_files.len());

        assert_eq!(txt_files.len(), 100, "Expected 100 .txt files, found {}", txt_files.len());
    }
}
