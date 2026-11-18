//! Integration tests verifying container isolation and security boundaries.

use alphen_core::ImageReference;
use alphen_runtime::{ContainerConfig, ResourceLimits, SecurityOptions};

#[test]
fn test_default_security_lockdown_configuration() {
    let cfg = ContainerConfig {
        id: "escape01test".into(),
        image_ref: ImageReference::parse("alpine:3.20").unwrap(),
        command: vec!["/bin/sh".into()],
        env: vec![],
        workdir: None,
        volumes: vec![],
        tmpfs: vec![],
        limits: ResourceLimits::default(),
        read_only: true, // Read-only rootfs prevents writes to host/rootfs
        hostname: Some("secure-box".into()),
        rm: true,
        detach: false,
        network_mode: alphen_net::NetworkMode::Bridge,
        ports: vec![],
        dns_servers: vec![],
        security: SecurityOptions {
            cap_add: vec![],
            cap_drop: vec!["ALL".into()],
            seccomp_profile: None,
            no_new_privs: true,
        },
    };

    assert!(cfg.read_only);
    assert!(cfg.security.no_new_privs);
    assert_eq!(cfg.security.cap_drop, vec!["ALL"]);
}

#[test]
fn test_capability_parsing_validation() {
    // Only valid capabilities allowed
    assert!(alphen_runtime::security::parse_capability("NET_BIND_SERVICE").is_ok());
    assert!(alphen_runtime::security::parse_capability("CHOWN").is_ok());
    assert!(alphen_runtime::security::parse_capability("SYS_ADMIN").is_ok());

    // Invalid/malicious capability inputs rejected
    assert!(alphen_runtime::security::parse_capability("ESCAPE_ROOT").is_err());
    assert!(alphen_runtime::security::parse_capability("../../../etc/shadow").is_err());
}

#[test]
fn test_security_opt_seccomp_parsing() {
    let opts = vec![
        "no-new-privs:true".to_string(),
        "seccomp=/etc/alphen-box/seccomp/default.json".to_string(),
    ];
    let seccomp = SecurityOptions::parse_security_opt(&opts).unwrap();
    assert_eq!(
        seccomp,
        Some("/etc/alphen-box/seccomp/default.json".to_string())
    );
}
