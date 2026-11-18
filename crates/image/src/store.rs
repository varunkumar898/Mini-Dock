#![forbid(unsafe_code)]

//! Content-addressed local image and layer blob store.

use crate::layer::extract_layer;
use crate::registry::RegistryClient;
use alphen_common::{AlphenError, Result};
use alphen_core::{default_blob_dir, default_image_dir, ImageReference, ImageSummary, StoredImage};
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Garbage collection summary report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcReport {
    /// Number of orphaned blobs removed.
    pub blobs_removed: usize,
    /// Total disk space freed in bytes.
    pub bytes_freed: u64,
}

/// Content-addressed local store managing images and layer blobs.
pub struct ImageStore {
    images_dir: PathBuf,
    blobs_dir: PathBuf,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new_default()
    }
}

impl ImageStore {
    /// Create an image store with default platform directories.
    pub fn new_default() -> Self {
        Self {
            images_dir: default_image_dir(),
            blobs_dir: default_blob_dir(),
        }
    }

    /// Create an image store with custom paths (useful for testing and sandboxing).
    pub fn new(images_dir: impl AsRef<Path>, blobs_dir: impl AsRef<Path>) -> Self {
        Self {
            images_dir: images_dir.as_ref().to_path_buf(),
            blobs_dir: blobs_dir.as_ref().to_path_buf(),
        }
    }

    /// Pull an image from an OCI/Docker registry into the local blob store.
    pub fn pull(&self, image_ref: &ImageReference) -> Result<StoredImage> {
        std::fs::create_dir_all(&self.images_dir)?;
        std::fs::create_dir_all(&self.blobs_dir)?;

        let mut client = RegistryClient::new();
        log::info!(
            "Authenticating with registry for '{}'...",
            image_ref.display_name()
        );
        client.authenticate(image_ref)?;

        log::info!("Fetching image manifest...");
        let (manifest, manifest_digest) = client.fetch_manifest(image_ref)?;

        log::info!("Fetching image configuration...");
        let config = client.fetch_config(image_ref, &manifest.config)?;

        // Strip "sha256:" prefix for storage paths
        let config_hash = manifest
            .config
            .digest
            .strip_prefix("sha256:")
            .unwrap_or(&manifest.config.digest);
        let config_blob_path = self.blobs_dir.join(config_hash);

        // Save config blob if not already stored
        if !config_blob_path.exists() {
            let config_json = serde_json::to_vec(&config)
                .map_err(|e| AlphenError::Image(format!("Failed to serialize config: {e}")))?;
            std::fs::write(&config_blob_path, config_json)?;
        }

        let mut layer_digests = Vec::new();
        let mut total_size = manifest.config.size;

        // Download and verify each layer blob
        let total_layers = manifest.layers.len();
        for (i, layer) in manifest.layers.iter().enumerate() {
            let layer_hash = layer
                .digest
                .strip_prefix("sha256:")
                .unwrap_or(&layer.digest);
            let blob_path = self.blobs_dir.join(layer_hash);
            total_size += layer.size;
            layer_digests.push(layer.digest.clone());

            if blob_path.exists() {
                log::info!(
                    "Layer {}/{} [{}] Already exists (deduplicated)",
                    i + 1,
                    total_layers,
                    &layer_hash[..12.min(layer_hash.len())]
                );
                continue;
            }

            log::info!(
                "Downloading layer {}/{} [{}] ({} bytes)...",
                i + 1,
                total_layers,
                &layer_hash[..12.min(layer_hash.len())],
                layer.size
            );

            client.download_blob_verified(image_ref, layer, &blob_path)?;
        }

        let id = config_hash[..12.min(config_hash.len())].to_string();
        let created = config
            .created
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let stored = StoredImage {
            reference: image_ref.clone(),
            id,
            manifest_digest,
            config_digest: manifest.config.digest.clone(),
            layers: layer_digests,
            size_bytes: total_size,
            created,
            config: config.config,
        };

        let image_file_path = self.images_dir.join(image_ref.storage_filename());
        let meta_json = serde_json::to_string_pretty(&stored)
            .map_err(|e| AlphenError::Image(format!("Failed to serialize image metadata: {e}")))?;
        std::fs::write(&image_file_path, meta_json)?;

        log::info!(
            "Pulled image: {} (ID: {})",
            image_ref.display_name(),
            stored.id
        );
        Ok(stored)
    }

    /// List all locally available container images.
    pub fn list_images(&self) -> Result<Vec<ImageSummary>> {
        if !self.images_dir.exists() {
            return Ok(Vec::new());
        }

        let mut summaries = Vec::new();
        let entries = std::fs::read_dir(&self.images_dir)?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(stored) = serde_json::from_str::<StoredImage>(&content) {
                        summaries.push(ImageSummary {
                            repository: stored.reference.repository,
                            tag: stored.reference.tag,
                            id: stored.id,
                            size_bytes: stored.size_bytes,
                            created: stored.created,
                        });
                    }
                }
            }
        }

        Ok(summaries)
    }

    /// Retrieve stored image metadata by image reference.
    pub fn get_image(&self, image_ref: &ImageReference) -> Result<Option<StoredImage>> {
        let path = self.images_dir.join(image_ref.storage_filename());
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)?;
        let stored = serde_json::from_str::<StoredImage>(&content)
            .map_err(|e| AlphenError::Image(format!("Corrupted image record: {e}")))?;
        Ok(Some(stored))
    }

    /// Remove an image reference tag.
    pub fn remove_image(&self, image_ref: &ImageReference) -> Result<bool> {
        let path = self.images_dir.join(image_ref.storage_filename());
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Unpack an image's layers in sequential order to produce a container root filesystem.
    pub fn unpack_rootfs(&self, stored: &StoredImage, target_rootfs: &Path) -> Result<()> {
        std::fs::create_dir_all(target_rootfs)?;

        for (i, layer_digest) in stored.layers.iter().enumerate() {
            let hash = layer_digest.strip_prefix("sha256:").unwrap_or(layer_digest);
            let blob_path = self.blobs_dir.join(hash);
            if !blob_path.exists() {
                return Err(AlphenError::Image(format!(
                    "Missing layer blob {} for image {}",
                    layer_digest, stored.id
                )));
            }

            log::debug!(
                "Extracting layer {}/{} [{}]",
                i + 1,
                stored.layers.len(),
                &hash[..12.min(hash.len())]
            );
            let file = File::open(&blob_path)?;
            extract_layer(file, target_rootfs)?;
        }

        Ok(())
    }

    /// Remove unreferenced layer and config blobs from the content-addressed blob store.
    pub fn gc(&self) -> Result<GcReport> {
        if !self.blobs_dir.exists() {
            return Ok(GcReport {
                blobs_removed: 0,
                bytes_freed: 0,
            });
        }

        // 1. Collect all active hashes referenced across all images
        let mut referenced_hashes = HashSet::new();
        if self.images_dir.exists() {
            for entry in std::fs::read_dir(&self.images_dir)?.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(img) = serde_json::from_str::<StoredImage>(&content) {
                            let cfg_hash = img
                                .config_digest
                                .strip_prefix("sha256:")
                                .unwrap_or(&img.config_digest);
                            referenced_hashes.insert(cfg_hash.to_string());
                            for l in img.layers {
                                let l_hash = l.strip_prefix("sha256:").unwrap_or(&l);
                                referenced_hashes.insert(l_hash.to_string());
                            }
                        }
                    }
                }
            }
        }

        // 2. Scan blobs directory and remove unreferenced files
        let mut blobs_removed = 0;
        let mut bytes_freed = 0;

        for entry in std::fs::read_dir(&self.blobs_dir)?.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
                    // Skip hidden or temp files
                    if file_name.starts_with('.') {
                        continue;
                    }

                    if !referenced_hashes.contains(file_name) {
                        if let Ok(meta) = path.metadata() {
                            bytes_freed += meta.len();
                        }
                        if let Ok(()) = std::fs::remove_file(&path) {
                            blobs_removed += 1;
                        }
                    }
                }
            }
        }

        Ok(GcReport {
            blobs_removed,
            bytes_freed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_cleans_unreferenced_blob() {
        let temp = std::env::temp_dir().join("alphen_test_gc");
        let _ = std::fs::remove_dir_all(&temp);
        let images = temp.join("images");
        let blobs = temp.join("blobs");
        std::fs::create_dir_all(&images).unwrap();
        std::fs::create_dir_all(&blobs).unwrap();

        let store = ImageStore::new(&images, &blobs);

        // Put an unreferenced dummy blob
        let orphan =
            blobs.join("1111222233334444555566667777888899990000aaaaabbbbccccddddeeeeffff");
        std::fs::write(&orphan, b"orphan payload data").unwrap();
        assert!(orphan.exists());

        let report = store.gc().unwrap();
        assert_eq!(report.blobs_removed, 1);
        assert_eq!(report.bytes_freed, 19);
        assert!(!orphan.exists());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
