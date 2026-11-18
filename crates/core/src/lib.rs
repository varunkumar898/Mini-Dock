#![forbid(unsafe_code)]

//! `alphen-core`: Fundamental types, specs, and data models for `alphen-box`.

use alphen_common::{AlphenError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Return default base data directory for alphen-box.
/// On Linux: `/var/lib/alphen-box`.
/// On Windows: `%ProgramData%\alphen-box` or `./data/alphen-box`.
pub fn default_data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ALPHEN_DATA_DIR") {
        return PathBuf::from(custom);
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, default to local `./data/alphen-box` for seamless non-admin & sandboxed use
        PathBuf::from("data").join("alphen-box")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("/var/lib/alphen-box")
    }
}

/// Return default volatile run directory for alphen-box.
/// On Linux: `/run/alphen-box`.
/// On Windows: `<data_dir>/run`.
pub fn default_run_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ALPHEN_RUN_DIR") {
        return PathBuf::from(custom);
    }

    #[cfg(target_os = "windows")]
    {
        default_data_dir().join("run")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("/run/alphen-box")
    }
}

/// Directory for content-addressed SHA-256 layer blobs.
pub fn default_blob_dir() -> PathBuf {
    default_data_dir().join("blobs").join("sha256")
}

/// Directory for stored image metadata manifests.
pub fn default_image_dir() -> PathBuf {
    default_data_dir().join("images")
}

/// Directory for persistent container configuration and metadata.
pub fn default_containers_dir() -> PathBuf {
    default_data_dir().join("containers")
}

/// Directory for network state and IPAM leases.
pub fn default_networks_dir() -> PathBuf {
    default_data_dir().join("networks")
}

/// Lifecycle status of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerStatus {
    Created,
    Running,
    Exited,
}

impl std::fmt::Display for ContainerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerStatus::Created => write!(f, "Created"),
            ContainerStatus::Running => write!(f, "Up"),
            ContainerStatus::Exited => write!(f, "Exited"),
        }
    }
}

/// Volatile and persistent container state record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerState {
    /// 12-character container ID.
    pub id: String,
    /// Container name.
    pub name: String,
    /// Container image reference string.
    pub image: String,
    /// Command array executed inside the container.
    pub command: Vec<String>,
    /// Current execution status.
    pub status: ContainerStatus,
    /// Host process ID of container init process if running.
    pub pid: Option<u32>,
    /// Process exit code if exited.
    pub exit_code: Option<i32>,
    /// Creation timestamp string.
    pub created: String,
    /// Time execution started.
    pub started_at: Option<String>,
    /// Time execution finished.
    pub finished_at: Option<String>,
}

impl ContainerState {
    /// Formatted status string matching Docker style (e.g. "Up 2 hours", "Exited (0) 5 minutes ago").
    pub fn formatted_status(&self) -> String {
        match self.status {
            ContainerStatus::Created => "Created".to_string(),
            ContainerStatus::Running => {
                if let Some(ref pid) = self.pid {
                    format!("Up (PID {pid})")
                } else {
                    "Up".to_string()
                }
            }
            ContainerStatus::Exited => {
                let code = self.exit_code.unwrap_or(0);
                format!("Exited ({code})")
            }
        }
    }

    /// Formatted command string truncated to 20 chars if long.
    pub fn formatted_command(&self) -> String {
        let joined = self.command.join(" ");
        if joined.len() > 20 {
            format!("\"{}...\"", &joined[..17])
        } else {
            format!("\"{joined}\"")
        }
    }
}

/// Basic representation of an image reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageReference {
    /// Registry host (e.g. docker.io, ghcr.io).
    pub registry: String,
    /// Repository / image name (e.g. library/alpine).
    pub repository: String,
    /// Tag (default: "latest") or digest.
    pub tag: String,
    /// Optional content digest (SHA-256).
    pub digest: Option<String>,
}

impl ImageReference {
    /// Parse an image string into an `ImageReference`.
    ///
    /// Examples:
    /// - `alpine:3.20` -> docker.io/library/alpine:3.20
    /// - `ghcr.io/org/repo:tag` -> ghcr.io/org/repo:tag
    pub fn parse(input: &str) -> Result<Self> {
        if input.trim().is_empty() {
            return Err(AlphenError::Config("Image name cannot be empty".into()));
        }

        let (image_part, digest) = match input.split_once('@') {
            Some((rest, d)) => (rest, Some(d.to_string())),
            None => (input, None),
        };

        let (repo_part, tag) = match image_part.rsplit_once(':') {
            Some((r, t)) if !t.contains('/') => (r, t.to_string()),
            _ => (image_part, "latest".to_string()),
        };

        let mut parts: Vec<&str> = repo_part.split('/').collect();
        let (registry, repository) = if parts.len() == 1 {
            ("docker.io".to_string(), format!("library/{}", parts[0]))
        } else if parts.len() == 2 && !parts[0].contains('.') && !parts[0].contains(':') {
            (
                "docker.io".to_string(),
                format!("{}/{}", parts[0], parts[1]),
            )
        } else {
            (parts.remove(0).to_string(), parts.join("/"))
        };

        Ok(Self {
            registry,
            repository,
            tag,
            digest,
        })
    }

    /// Return the canonical reference string (e.g. `docker.io/library/alpine:3.20`).
    pub fn canonical(&self) -> String {
        format!("{}/{}:{}", self.registry, self.repository, self.tag)
    }

    /// Return a human-readable display string (e.g. `alpine:3.20`).
    pub fn display_name(&self) -> String {
        if self.registry == "docker.io" {
            if let Some(short) = self.repository.strip_prefix("library/") {
                return format!("{}:{}", short, self.tag);
            }
            return format!("{}:{}", self.repository, self.tag);
        }
        format!("{}/{}:{}", self.registry, self.repository, self.tag)
    }

    /// Return a safe file system key name (e.g. `docker.io_library_alpine_3.20.json`).
    pub fn storage_filename(&self) -> String {
        let safe_repo = self.repository.replace('/', "_");
        format!("{}_{}_{}.json", self.registry, safe_repo, self.tag)
    }
}

/// Metadata summary of a locally stored container image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSummary {
    /// Full image repository name.
    pub repository: String,
    /// Image tag.
    pub tag: String,
    /// Short SHA-256 image ID (first 12 characters).
    pub id: String,
    /// Total virtual size on disk in bytes.
    pub size_bytes: u64,
    /// Creation timestamp string or ISO timestamp.
    pub created: String,
}

impl ImageSummary {
    /// Formatted human-readable size (e.g. "7.8 MB", "124 KB").
    pub fn formatted_size(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;

        if self.size_bytes >= GB {
            format!("{:.1} GB", self.size_bytes as f64 / GB as f64)
        } else if self.size_bytes >= MB {
            format!("{:.1} MB", self.size_bytes as f64 / MB as f64)
        } else if self.size_bytes >= KB {
            format!("{:.1} KB", self.size_bytes as f64 / KB as f64)
        } else {
            format!("{} B", self.size_bytes)
        }
    }
}

/// Fully described local image record stored in `images/<filename>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredImage {
    /// Canonical image reference.
    pub reference: ImageReference,
    /// Image ID (short or full SHA-256 digest of config).
    pub id: String,
    /// Digest of the image manifest.
    pub manifest_digest: String,
    /// Digest of the config blob.
    pub config_digest: String,
    /// Array of layer digests in rootfs stack order (bottom layer first).
    pub layers: Vec<String>,
    /// Aggregated layer size in bytes.
    pub size_bytes: u64,
    /// Image created timestamp.
    pub created: String,
    /// Container runtime configuration details.
    pub config: Option<OciConfigDetails>,
}

/// Container runtime execution parameters from image config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciConfigDetails {
    /// Environment variables (KEY=VALUE).
    #[serde(default)]
    pub env: Vec<String>,
    /// Command arguments.
    #[serde(default)]
    pub cmd: Vec<String>,
    /// Entrypoint executable.
    #[serde(default)]
    pub entrypoint: Vec<String>,
    /// Working directory inside container.
    pub working_dir: Option<String>,
    /// User or UID/GID.
    pub user: Option<String>,
}

/// OCI content descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciDescriptor {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
}

/// Platform specification in OCI index / manifest list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciPlatform {
    pub architecture: String,
    pub os: String,
    #[serde(default)]
    pub variant: Option<String>,
}

/// OCI Image Index / Docker Manifest List item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciManifestEntry {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    pub platform: Option<OciPlatform>,
}

/// OCI Image Index / Docker Manifest List.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciImageIndex {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(default)]
    pub manifests: Vec<OciManifestEntry>,
}

/// OCI Image Manifest / Docker v2 Schema 2 Manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType", default)]
    pub media_type: Option<String>,
    pub config: OciDescriptor,
    pub layers: Vec<OciDescriptor>,
}

/// OCI Image Config JSON representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciImageConfig {
    pub architecture: String,
    pub os: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub config: Option<OciConfigDetails>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_reference_parse_docker_hub_official() {
        let r = ImageReference::parse("alpine:3.20").unwrap();
        assert_eq!(r.registry, "docker.io");
        assert_eq!(r.repository, "library/alpine");
        assert_eq!(r.tag, "3.20");
        assert_eq!(r.display_name(), "alpine:3.20");
        assert_eq!(r.storage_filename(), "docker.io_library_alpine_3.20.json");
    }

    #[test]
    fn test_image_reference_parse_custom_registry() {
        let r = ImageReference::parse("ghcr.io/alphen/box:latest").unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "alphen/box");
        assert_eq!(r.tag, "latest");
        assert_eq!(r.display_name(), "ghcr.io/alphen/box:latest");
        assert_eq!(r.storage_filename(), "ghcr.io_alphen_box_latest.json");
    }

    #[test]
    fn test_formatted_size() {
        let summary = ImageSummary {
            repository: "alpine".into(),
            tag: "3.20".into(),
            id: "a1b2c3d4e5f6".into(),
            size_bytes: 8 * 1024 * 1024,
            created: "2 weeks ago".into(),
        };
        assert_eq!(summary.formatted_size(), "8.0 MB");
    }

    #[test]
    fn test_default_paths_exist() {
        let base = default_data_dir();
        assert!(!base.as_os_str().is_empty());
        let blobs = default_blob_dir();
        assert!(blobs.ends_with(PathBuf::from("blobs").join("sha256")));
    }
}
