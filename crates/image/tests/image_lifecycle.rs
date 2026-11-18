//! Integration tests for Milestone 2: Image storage, deduplication, whiteouts, and GC

use alphen_core::{ImageReference, OciConfigDetails, StoredImage};
use alphen_image::ImageStore;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use tar::Builder;

#[test]
fn test_image_lifecycle_deduplication_and_gc() {
    let temp = std::env::temp_dir().join("alphen_test_m2_lifecycle");
    let _ = std::fs::remove_dir_all(&temp);
    let images_dir = temp.join("images");
    let blobs_dir = temp.join("blobs");
    let rootfs_dir = temp.join("rootfs");
    std::fs::create_dir_all(&images_dir).unwrap();
    std::fs::create_dir_all(&blobs_dir).unwrap();

    let store = ImageStore::new(&images_dir, &blobs_dir);

    // 1. Create a dummy base layer blob: contains /bin/sh and /etc/os-release
    let mut base_tar = Vec::new();
    {
        let mut builder = Builder::new(&mut base_tar);
        let mut h1 = tar::Header::new_gnu();
        h1.set_path("bin/sh").unwrap();
        h1.set_size(12);
        h1.set_cksum();
        builder.append(&h1, Cursor::new(b"#!/bin/sh\n")).unwrap();

        let mut h2 = tar::Header::new_gnu();
        h2.set_path("etc/os-release").unwrap();
        h2.set_size(17);
        h2.set_cksum();
        builder
            .append(&h2, Cursor::new(b"NAME=\"AlphenOS\"\n"))
            .unwrap();

        builder.finish().unwrap();
    }

    let mut gz_base = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut gz_base, Compression::default());
        std::io::copy(&mut Cursor::new(&base_tar), &mut encoder).unwrap();
    }

    let base_digest = format!("sha256:{:x}", Sha256::digest(&gz_base));
    let base_hash = base_digest.strip_prefix("sha256:").unwrap();
    std::fs::write(blobs_dir.join(base_hash), &gz_base).unwrap();

    // 2. Create a top layer blob with an added file /app/server and a whiteout .wh.os-release
    let mut top_tar = Vec::new();
    {
        let mut builder = Builder::new(&mut top_tar);
        let mut h1 = tar::Header::new_gnu();
        h1.set_path("app/server").unwrap();
        h1.set_size(10);
        h1.set_cksum();
        builder.append(&h1, Cursor::new(b"server-bin")).unwrap();

        let mut h_wh = tar::Header::new_gnu();
        h_wh.set_path("etc/.wh.os-release").unwrap();
        h_wh.set_size(0);
        h_wh.set_cksum();
        builder.append(&h_wh, Cursor::new(&[])).unwrap();

        builder.finish().unwrap();
    }

    let top_digest = format!("sha256:{:x}", Sha256::digest(&top_tar));
    let top_hash = top_digest.strip_prefix("sha256:").unwrap();
    std::fs::write(blobs_dir.join(top_hash), &top_tar).unwrap();

    // 3. Register Image 1: alpine:3.20 (uses base_digest)
    let img1_ref = ImageReference::parse("alpine:3.20").unwrap();
    let img1 = StoredImage {
        reference: img1_ref.clone(),
        id: "111111111111".into(),
        manifest_digest: "sha256:manifest1".into(),
        config_digest: "sha256:config1".into(),
        layers: vec![base_digest.clone()],
        size_bytes: gz_base.len() as u64,
        created: "2026-11-18".into(),
        config: Some(OciConfigDetails {
            cmd: vec!["/bin/sh".into()],
            ..Default::default()
        }),
    };
    std::fs::write(
        images_dir.join(img1_ref.storage_filename()),
        serde_json::to_string_pretty(&img1).unwrap(),
    )
    .unwrap();

    // 4. Register Image 2: myapp:v1 (shares base_digest, plus top_digest)
    let img2_ref = ImageReference::parse("myapp:v1").unwrap();
    let img2 = StoredImage {
        reference: img2_ref.clone(),
        id: "222222222222".into(),
        manifest_digest: "sha256:manifest2".into(),
        config_digest: "sha256:config2".into(),
        layers: vec![base_digest.clone(), top_digest.clone()],
        size_bytes: (gz_base.len() + top_tar.len()) as u64,
        created: "2026-11-18".into(),
        config: Some(OciConfigDetails {
            cmd: vec!["/app/server".into()],
            ..Default::default()
        }),
    };
    std::fs::write(
        images_dir.join(img2_ref.storage_filename()),
        serde_json::to_string_pretty(&img2).unwrap(),
    )
    .unwrap();

    // 5. Test store.list_images()
    let images = store.list_images().unwrap();
    assert_eq!(images.len(), 2);

    // 6. Test unpacking rootfs for Image 2 and verify whiteout worked
    store.unpack_rootfs(&img2, &rootfs_dir).unwrap();
    assert!(rootfs_dir.join("bin/sh").exists());
    assert!(rootfs_dir.join("app/server").exists());
    // /etc/os-release was whited out by layer 2
    assert!(!rootfs_dir.join("etc/os-release").exists());

    // 7. Add an unreferenced orphan blob
    let orphan_hash = "9999999999999999999999999999999999999999999999999999999999999999";
    std::fs::write(blobs_dir.join(orphan_hash), b"orphan content").unwrap();

    // 8. Run GC: orphan blob must be removed, shared layers must remain intact!
    let gc_report = store.gc().unwrap();
    assert_eq!(gc_report.blobs_removed, 1);
    assert_eq!(gc_report.bytes_freed, 14);
    assert!(!blobs_dir.join(orphan_hash).exists());
    assert!(blobs_dir.join(base_hash).exists());
    assert!(blobs_dir.join(top_hash).exists());

    // 9. Remove Image 1
    assert!(store.remove_image(&img1_ref).unwrap());
    let remaining = store.list_images().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].repository, "library/myapp");
    assert_eq!(remaining[0].tag, "v1");

    let _ = std::fs::remove_dir_all(&temp);
}
