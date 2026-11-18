//! Integration tests for Docker CLI compatibility and unsupported flag warnings.

use std::process::Command;

#[test]
fn test_docker_version_compatibility() {
    let target_bin = if cfg!(target_os = "windows") {
        "target/release/docker.exe"
    } else {
        "target/release/docker"
    };

    if !std::path::Path::new(target_bin).exists() {
        return; // Skip if release binary not compiled in this test run
    }

    let output = Command::new(target_bin)
        .arg("version")
        .output()
        .expect("Failed to execute docker wrapper");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alphen-box"));
}

#[test]
fn test_docker_unsupported_flag_warning() {
    let target_bin = if cfg!(target_os = "windows") {
        "target/release/docker.exe"
    } else {
        "target/release/docker"
    };

    if !std::path::Path::new(target_bin).exists() {
        return;
    }

    let output = Command::new(target_bin)
        .args(["version", "--restart=always"])
        .output()
        .expect("Failed to execute docker wrapper");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "warning: flag '--restart=always' is not supported in alphen-box v1 and was ignored"
    ));
}
