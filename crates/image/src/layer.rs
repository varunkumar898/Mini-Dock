#![forbid(unsafe_code)]

//! Layer decompression and OCI whiteout file handling.

use alphen_common::{AlphenError, Result};
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::Path;
use tar::Archive;

/// Extract a layer archive (gzip-compressed or raw tar) into `target_dir`.
///
/// Implements OCI whiteout specifications:
/// - `.wh.<name>`: removes `<target_dir>/<name>`
/// - `.wh..wh..opq`: opaque whiteout marker; removes all existing children in the containing directory
pub fn extract_layer<R: Read>(mut reader: R, target_dir: &Path) -> Result<()> {
    // Read the first 2 bytes to detect gzip magic number 0x1f, 0x8b
    let mut header = [0u8; 2];
    let n = reader.read(&mut header)?;
    if n == 0 {
        return Ok(());
    }

    let chained = std::io::Cursor::new(header[..n].to_vec()).chain(reader);

    if n >= 2 && header[0] == 0x1f && header[1] == 0x8b {
        let gz = GzDecoder::new(chained);
        unpack_tar(gz, target_dir)
    } else {
        unpack_tar(chained, target_dir)
    }
}

fn unpack_tar<R: Read>(reader: R, target_dir: &Path) -> Result<()> {
    let mut archive = Archive::new(reader);
    std::fs::create_dir_all(target_dir)?;

    let canonical_target = target_dir
        .canonicalize()
        .map_err(|e| AlphenError::Runtime(format!("Failed to canonicalize target dir: {e}")))?;

    for entry_res in archive.entries()? {
        let mut entry = entry_res?;
        let raw_path = entry.path()?.to_path_buf();

        // Check for directory traversal
        let mut safe_components = Vec::new();
        for comp in raw_path.components() {
            match comp {
                std::path::Component::Normal(c) => safe_components.push(c),
                std::path::Component::CurDir => {}
                _ => {
                    // Ignore root or parent components to prevent escape
                }
            }
        }

        if safe_components.is_empty() {
            continue;
        }

        let file_name = safe_components
            .last()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        // 1. OCI Opaque Whiteout: .wh..wh..opq
        // When present in a directory, all children of that directory from lower layers are hidden.
        if file_name == ".wh..wh..opq" {
            let parent_dir = if safe_components.len() > 1 {
                let mut p = canonical_target.clone();
                for comp in &safe_components[..safe_components.len() - 1] {
                    p.push(comp);
                }
                p
            } else {
                canonical_target.clone()
            };

            if parent_dir.exists() && parent_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&parent_dir) {
                    for e in entries.flatten() {
                        let path = e.path();
                        if path.is_dir() {
                            let _ = std::fs::remove_dir_all(&path);
                        } else {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }
            continue;
        }

        // 2. OCI Explicit Whiteout: .wh.<basename>
        // Deletes the specified file/dir from lower layers.
        if let Some(target_to_delete) = file_name.strip_prefix(".wh.") {
            let mut del_path = canonical_target.clone();
            for comp in &safe_components[..safe_components.len() - 1] {
                del_path.push(comp);
            }
            del_path.push(target_to_delete);

            if del_path.exists() {
                if del_path.is_dir() {
                    let _ = std::fs::remove_dir_all(&del_path);
                } else {
                    let _ = std::fs::remove_file(&del_path);
                }
            }
            continue;
        }

        // 3. Normal file/dir entry
        let mut dest_path = canonical_target.clone();
        for comp in &safe_components {
            dest_path.push(comp);
        }

        // Safety verification: ensure destination path is strictly within canonical_target
        if !dest_path.starts_with(&canonical_target) {
            log::warn!("Skipping path traversal attempt in archive: {:?}", raw_path);
            continue;
        }

        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = entry.unpack(&dest_path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tar::Builder;

    #[test]
    fn test_whiteout_file_removal() {
        let temp = std::env::temp_dir().join("alphen_test_whiteout");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        // 1. Create a dummy file
        let existing = temp.join("victim.txt");
        std::fs::write(&existing, b"to be removed").unwrap();
        assert!(existing.exists());

        // 2. Build a tar stream with `.wh.victim.txt`
        let mut tar_data = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_data);
            let mut header = tar::Header::new_gnu();
            header.set_path(".wh.victim.txt").unwrap();
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, Cursor::new(&[])).unwrap();
            builder.finish().unwrap();
        }

        // 3. Extract and verify victim.txt is gone
        extract_layer(Cursor::new(tar_data), &temp).unwrap();
        assert!(!existing.exists());

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_opaque_whiteout_removal() {
        let temp = std::env::temp_dir().join("alphen_test_opaque");
        let _ = std::fs::remove_dir_all(&temp);
        let sub = temp.join("mydir");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("old1.txt"), b"old1").unwrap();
        std::fs::write(sub.join("old2.txt"), b"old2").unwrap();

        // Build a tar with mydir/.wh..wh..opq and a new file mydir/new.txt
        let mut tar_data = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_data);

            let mut opq_header = tar::Header::new_gnu();
            opq_header.set_path("mydir/.wh..wh..opq").unwrap();
            opq_header.set_size(0);
            opq_header.set_cksum();
            builder.append(&opq_header, Cursor::new(&[])).unwrap();

            let mut new_header = tar::Header::new_gnu();
            new_header.set_path("mydir/new.txt").unwrap();
            new_header.set_size(3);
            new_header.set_cksum();
            builder.append(&new_header, Cursor::new(b"new")).unwrap();

            builder.finish().unwrap();
        }

        extract_layer(Cursor::new(tar_data), &temp).unwrap();

        assert!(!sub.join("old1.txt").exists());
        assert!(!sub.join("old2.txt").exists());
        assert!(sub.join("new.txt").exists());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
