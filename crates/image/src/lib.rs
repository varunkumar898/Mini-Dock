#![forbid(unsafe_code)]

//! `alphen-image`: OCI image store and registry distribution client.

use alphen_common::Result;
use alphen_core::ImageSummary;
use std::path::{Path, PathBuf};

/// Default image storage directory on Alphen OS.
pub const DEFAULT_IMAGE_STORE_DIR: &str = "/var/lib/alphen-box/images";

/// Blob store directory for content-addressed SHA-256 layers.
pub const DEFAULT_BLOB_STORE_DIR: &str = "/var/lib/alphen-box/blobs";

/// Local image store interface.
pub struct ImageStore {
    base_dir: PathBuf,
}

impl ImageStore {
    /// Initialize image store at a given path.
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    /// List all locally cached images.
    pub fn list_images(&self) -> Result<Vec<ImageSummary>> {
        // If directory doesn't exist yet, return empty list gracefully
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        let entries = std::fs::read_dir(&self.base_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(summary) = serde_json::from_str::<ImageSummary>(&data) {
                        summaries.push(summary);
                    }
                }
            }
        }

        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_store_listing() {
        let store = ImageStore::new("non_existent_dir_for_test");
        let list = store.list_images().unwrap();
        assert!(list.is_empty());
    }
}
