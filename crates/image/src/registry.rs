#![forbid(unsafe_code)]

//! OCI Distribution Spec registry client for downloading images and verifying digests.

use alphen_common::{AlphenError, Result};
use alphen_core::{ImageReference, OciDescriptor, OciImageConfig, OciImageIndex, OciManifest};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use ureq::Agent;

const OCI_MANIFEST_MEDIA_TYPES: &str = "\
    application/vnd.oci.image.manifest.v1+json, \
    application/vnd.docker.distribution.manifest.v2+json, \
    application/vnd.oci.image.index.v1+json, \
    application/vnd.docker.distribution.manifest.list.v2+json";

/// Client for communicating with OCI and Docker v2 registries.
pub struct RegistryClient {
    agent: Agent,
    auth_token: Option<String>,
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryClient {
    /// Create a new registry client instance using rustls TLS backend.
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout_read(std::time::Duration::from_secs(60))
            .build();

        Self {
            agent,
            auth_token: None,
        }
    }

    /// Obtain authentication token for a given repository pull scope.
    pub fn authenticate(&mut self, image_ref: &ImageReference) -> Result<()> {
        let host = if image_ref.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &image_ref.registry
        };

        let check_url = format!("https://{host}/v2/");
        let resp = self.agent.get(&check_url).call();

        match resp {
            Ok(_) => {
                // Public registry without auth gate
                self.auth_token = None;
                Ok(())
            }
            Err(ureq::Error::Status(401, response)) => {
                let www_auth = response.header("www-authenticate").unwrap_or("");
                self.request_token(www_auth, image_ref)
            }
            Err(e) => Err(AlphenError::Image(format!(
                "Failed to contact registry '{host}': {e}"
            ))),
        }
    }

    fn request_token(&mut self, auth_header: &str, image_ref: &ImageReference) -> Result<()> {
        // Parse Bearer realm="...",service="...",scope="..."
        if !auth_header.starts_with("Bearer ") {
            // Docker Hub fallback
            if image_ref.registry == "docker.io" {
                let token_url = format!(
                    "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
                    image_ref.repository
                );
                return self.fetch_bearer_token(&token_url);
            }
            return Ok(());
        }

        let params = &auth_header["Bearer ".len()..];
        let mut realm = None;
        let mut service = None;

        for part in params.split(',') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                let val = v.trim_matches('"');
                if k == "realm" {
                    realm = Some(val);
                } else if k == "service" {
                    service = Some(val);
                }
            }
        }

        let realm = realm
            .ok_or_else(|| AlphenError::Image("Missing realm in Www-Authenticate header".into()))?;

        let mut token_url = format!("{realm}?scope=repository:{}:pull", image_ref.repository);
        if let Some(srv) = service {
            token_url.push_str(&format!("&service={srv}"));
        }

        self.fetch_bearer_token(&token_url)
    }

    fn fetch_bearer_token(&mut self, token_url: &str) -> Result<()> {
        let resp = self
            .agent
            .get(token_url)
            .call()
            .map_err(|e| AlphenError::Image(format!("Auth token request failed: {e}")))?;

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            token: Option<String>,
            access_token: Option<String>,
        }

        let token_data: TokenResponse = serde_json::from_reader(resp.into_reader())
            .map_err(|e| AlphenError::Image(format!("Invalid auth token response: {e}")))?;

        self.auth_token = token_data.token.or(token_data.access_token);
        Ok(())
    }

    fn get_request(&self, url: &str) -> ureq::Request {
        let mut req = self.agent.get(url);
        if let Some(token) = &self.auth_token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        req
    }

    /// Fetch image manifest, automatically resolving multi-architecture index/lists for ARM64.
    pub fn fetch_manifest(&mut self, image_ref: &ImageReference) -> Result<(OciManifest, String)> {
        let host = if image_ref.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &image_ref.registry
        };

        let target_reference = image_ref.digest.as_deref().unwrap_or(&image_ref.tag);
        let manifest_url = format!(
            "https://{host}/v2/{}/manifests/{target_reference}",
            image_ref.repository
        );

        let resp = self
            .get_request(&manifest_url)
            .set("Accept", OCI_MANIFEST_MEDIA_TYPES)
            .call()
            .map_err(|e| {
                AlphenError::Image(format!(
                    "Failed to fetch manifest from '{manifest_url}': {e}"
                ))
            })?;

        let raw_manifest_bytes = {
            let mut buf = Vec::new();
            resp.into_reader()
                .read_to_end(&mut buf)
                .map_err(|e| AlphenError::Image(format!("Failed to read manifest payload: {e}")))?;
            buf
        };

        let digest = format!("sha256:{:x}", Sha256::digest(&raw_manifest_bytes));

        // 1. Try parsing as OCI Image Index / Docker Manifest List
        if let Ok(index) = serde_json::from_slice::<OciImageIndex>(&raw_manifest_bytes) {
            if !index.manifests.is_empty() {
                log::debug!(
                    "Found multi-architecture index with {} manifests",
                    index.manifests.len()
                );

                // Find ARM64 / linux entry
                let chosen = index
                    .manifests
                    .iter()
                    .find(|m| {
                        if let Some(p) = &m.platform {
                            p.os == "linux"
                                && (p.architecture == "arm64" || p.architecture == "aarch64")
                        } else {
                            false
                        }
                    })
                    .or_else(|| {
                        // Fallback to armv7 if present
                        index.manifests.iter().find(|m| {
                            if let Some(p) = &m.platform {
                                p.os == "linux" && p.architecture == "arm"
                            } else {
                                false
                            }
                        })
                    })
                    .or_else(|| index.manifests.first());

                if let Some(target) = chosen {
                    log::info!(
                        "Selected manifest {} for platform: {:?}",
                        target.digest,
                        target.platform
                    );
                    let mut child_ref = image_ref.clone();
                    child_ref.digest = Some(target.digest.clone());
                    return self.fetch_manifest(&child_ref);
                }
            }
        }

        // 2. Parse as standard OCI / Docker v2 Schema 2 Manifest
        let manifest: OciManifest = serde_json::from_slice(&raw_manifest_bytes)
            .map_err(|e| AlphenError::Image(format!("Failed to parse image manifest JSON: {e}")))?;

        Ok((manifest, digest))
    }

    /// Download a blob directly to a destination path, verifying its SHA-256 digest in real time.
    pub fn download_blob_verified(
        &self,
        image_ref: &ImageReference,
        descriptor: &OciDescriptor,
        dest_path: &Path,
    ) -> Result<()> {
        let host = if image_ref.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &image_ref.registry
        };

        let blob_url = format!(
            "https://{host}/v2/{}/blobs/{}",
            image_ref.repository, descriptor.digest
        );

        let resp = self.get_request(&blob_url).call().map_err(|e| {
            AlphenError::Image(format!(
                "Failed to download blob '{}': {e}",
                descriptor.digest
            ))
        })?;

        let mut reader = resp.into_reader();
        let parent = dest_path.parent().unwrap();
        std::fs::create_dir_all(parent)?;

        // Write to temporary file first to ensure atomic placement
        let temp_path = parent.join(format!(".tmp-{}", descriptor.digest.replace(':', "_")));
        let mut file = std::fs::File::create(&temp_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
            file.write_all(&buffer[..bytes_read])?;
        }
        file.flush()?;
        drop(file);

        let calculated_digest = format!("sha256:{:x}", hasher.finalize());
        if calculated_digest != descriptor.digest {
            let _ = std::fs::remove_file(&temp_path);
            return Err(AlphenError::Image(format!(
                "Digest verification failure: expected {}, got {}",
                descriptor.digest, calculated_digest
            )));
        }

        std::fs::rename(&temp_path, dest_path)?;
        Ok(())
    }

    /// Download and parse image configuration JSON.
    pub fn fetch_config(
        &self,
        image_ref: &ImageReference,
        descriptor: &OciDescriptor,
    ) -> Result<OciImageConfig> {
        let host = if image_ref.registry == "docker.io" {
            "registry-1.docker.io"
        } else {
            &image_ref.registry
        };

        let config_url = format!(
            "https://{host}/v2/{}/blobs/{}",
            image_ref.repository, descriptor.digest
        );

        let resp = self.get_request(&config_url).call().map_err(|e| {
            AlphenError::Image(format!(
                "Failed to download config '{}': {e}",
                descriptor.digest
            ))
        })?;

        let config: OciImageConfig = serde_json::from_reader(resp.into_reader())
            .map_err(|e| AlphenError::Image(format!("Failed to parse OCI config JSON: {e}")))?;

        Ok(config)
    }
}
