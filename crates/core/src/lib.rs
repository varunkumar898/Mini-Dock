#![forbid(unsafe_code)]

//! `alphen-core`: Fundamental types, specs, and data models for `alphen-box`.

use serde::{Deserialize, Serialize};

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
    pub fn parse(input: &str) -> Result<Self, alphen_common::AlphenError> {
        if input.trim().is_empty() {
            return Err(alphen_common::AlphenError::Config(
                "Image name cannot be empty".into(),
            ));
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
    }

    #[test]
    fn test_image_reference_parse_custom_registry() {
        let r = ImageReference::parse("ghcr.io/alphen/box:latest").unwrap();
        assert_eq!(r.registry, "ghcr.io");
        assert_eq!(r.repository, "alphen/box");
        assert_eq!(r.tag, "latest");
        assert_eq!(r.display_name(), "ghcr.io/alphen/box:latest");
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
}
