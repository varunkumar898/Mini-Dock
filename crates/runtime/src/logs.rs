//! Container log capture and size-capped log rotation (10 MB x 3 files).

use alphen_common::Result;
use alphen_core::default_containers_dir;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const MAX_LOG_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB cap per log segment
const MAX_BACKUP_FILES: usize = 3;

/// Writer that streams output and rotates log files when exceeding 10 MB.
pub struct RotatingLogWriter {
    base_path: PathBuf,
    current_file: File,
    current_size: u64,
}

impl RotatingLogWriter {
    /// Create or append to the container's log file.
    pub fn new(container_id: &str) -> Result<Self> {
        let logs_dir = default_containers_dir().join(container_id).join("logs");
        std::fs::create_dir_all(&logs_dir)?;
        let base_path = logs_dir.join("stdout.log");

        let current_size = if base_path.exists() {
            base_path.metadata()?.len()
        } else {
            0
        };

        let current_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&base_path)?;

        Ok(Self {
            base_path,
            current_file,
            current_size,
        })
    }

    /// Check if rotation is required and rotate log files:
    /// stdout.log.2 -> stdout.log.3
    /// stdout.log.1 -> stdout.log.2
    /// stdout.log   -> stdout.log.1
    fn rotate_if_needed(&mut self, incoming_bytes: usize) -> Result<()> {
        if self.current_size + (incoming_bytes as u64) < MAX_LOG_SIZE_BYTES {
            return Ok(());
        }

        // Flush and close current file
        let _ = self.current_file.flush();

        for i in (1..MAX_BACKUP_FILES).rev() {
            let src = self.base_path.with_extension(format!("log.{i}"));
            let dst = self.base_path.with_extension(format!("log.{}", i + 1));
            if src.exists() {
                let _ = std::fs::rename(src, dst);
            }
        }

        let first_backup = self.base_path.with_extension("log.1");
        if self.base_path.exists() {
            let _ = std::fs::rename(&self.base_path, first_backup);
        }

        self.current_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.base_path)?;
        self.current_size = 0;
        Ok(())
    }
}

impl Write for RotatingLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Err(e) = self.rotate_if_needed(buf.len()) {
            log::warn!("Log rotation error: {e}");
        }

        let written = self.current_file.write(buf)?;
        self.current_size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.current_file.flush()
    }
}

/// Print logs for a given container, optionally following output.
pub fn read_container_logs(container_id: &str, follow: bool) -> Result<()> {
    let log_file_path = default_containers_dir()
        .join(container_id)
        .join("logs")
        .join("stdout.log");

    if !log_file_path.exists() {
        return Ok(());
    }

    let file = File::open(&log_file_path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes > 0 {
            print!("{line}");
        } else if follow {
            std::thread::sleep(std::time::Duration::from_millis(150));
        } else {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotating_log_writer_creation() {
        let temp = std::env::temp_dir().join("alphen_test_logs");
        let _ = std::fs::remove_dir_all(&temp);
        std::env::set_var("ALPHEN_DATA_DIR", &temp);

        let mut writer = RotatingLogWriter::new("test-c1").unwrap();
        writer.write_all(b"Hello from container log\n").unwrap();
        writer.flush().unwrap();

        let log_path = temp.join("containers/test-c1/logs/stdout.log");
        assert!(log_path.exists());
        let content = std::fs::read_to_string(log_path).unwrap();
        assert_eq!(content, "Hello from container log\n");

        let _ = std::fs::remove_dir_all(&temp);
        std::env::remove_var("ALPHEN_DATA_DIR");
    }
}
