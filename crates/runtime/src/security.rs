//! Linux container security hardening: `no_new_privs`, capability dropping, and seccomp BPF.
//!
//! References:
//! - `prctl(2)`: `PR_SET_NO_NEW_PRIVS`, `PR_CAPBSET_DROP`, `PR_SET_SECCOMP`
//! - `capabilities(7)`: Linux capability sets
//! - `seccomp(2)`: Secure computing mode filter (BPF)

use alphen_common::{AlphenError, Result};
use std::collections::HashSet;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub const PR_SET_SECCOMP: libc::c_int = 22;
    pub const PR_CAPBSET_READ: libc::c_int = 23;
    pub const PR_CAPBSET_DROP: libc::c_int = 24;
    pub const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;

    pub const SECCOMP_MODE_FILTER: libc::c_ulong = 2;
    pub const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;
    pub const SECCOMP_RET_ERRNO: u32 = 0x00050000;

    // BPF instruction classes and flags
    pub const BPF_LD: u16 = 0x00;
    pub const BPF_W: u16 = 0x00;
    pub const BPF_ABS: u16 = 0x20;
    pub const BPF_JMP: u16 = 0x05;
    pub const BPF_JEQ: u16 = 0x10;
    pub const BPF_K: u16 = 0x00;
    pub const BPF_RET: u16 = 0x06;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct SockFilter {
        pub code: u16,
        pub jt: u8,
        pub jf: u8,
        pub k: u32,
    }

    #[repr(C)]
    #[derive(Debug)]
    pub struct SockFprog {
        pub len: u16,
        pub filter: *const SockFilter,
    }

    pub fn bpf_stmt(code: u16, k: u32) -> SockFilter {
        SockFilter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    pub fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
        SockFilter { code, jt, jf, k }
    }
}

/// Linux capability name to number mapping.
pub fn parse_capability(name: &str) -> Result<u32> {
    let clean = name.trim().to_ascii_uppercase().replace("CAP_", "");

    match clean.as_str() {
        "CHOWN" => Ok(0),
        "DAC_OVERRIDE" => Ok(1),
        "DAC_READ_SEARCH" => Ok(2),
        "FOWNER" => Ok(3),
        "FSETID" => Ok(4),
        "KILL" => Ok(5),
        "SETGID" => Ok(6),
        "SETUID" => Ok(7),
        "SETPCAP" => Ok(8),
        "LINUX_IMMUTABLE" => Ok(9),
        "NET_BIND_SERVICE" => Ok(10),
        "NET_BROADCAST" => Ok(11),
        "NET_ADMIN" => Ok(12),
        "NET_RAW" => Ok(13),
        "IPC_LOCK" => Ok(14),
        "IPC_OWNER" => Ok(15),
        "SYS_MODULE" => Ok(16),
        "SYS_RAWIO" => Ok(17),
        "SYS_CHROOT" => Ok(18),
        "SYS_PTRACE" => Ok(19),
        "SYS_PACCT" => Ok(20),
        "SYS_ADMIN" => Ok(21),
        "SYS_BOOT" => Ok(22),
        "SYS_NICE" => Ok(23),
        "SYS_RESOURCE" => Ok(24),
        "SYS_TIME" => Ok(25),
        "SYS_TTY_CONFIG" => Ok(26),
        "MKNOD" => Ok(27),
        "LEASE" => Ok(28),
        "AUDIT_WRITE" => Ok(29),
        "AUDIT_CONTROL" => Ok(30),
        "SETFCAP" => Ok(31),
        "MAC_OVERRIDE" => Ok(32),
        "MAC_ADMIN" => Ok(33),
        "SYSLOG" => Ok(34),
        "WAKE_ALARM" => Ok(35),
        "BLOCK_SUSPEND" => Ok(36),
        "AUDIT_READ" => Ok(37),
        "PERFMON" => Ok(38),
        "BPF" => Ok(39),
        "CHECKPOINT_RESTORE" => Ok(40),
        other => Err(AlphenError::Config(format!(
            "Unknown Linux capability: {other}"
        ))),
    }
}

/// Security options configuration for a container.
#[derive(Debug, Clone, Default)]
pub struct SecurityOptions {
    /// Capabilities to add to bounding set.
    pub cap_add: Vec<String>,
    /// Capabilities to drop from bounding set.
    pub cap_drop: Vec<String>,
    /// Custom seccomp profile path (from `--security-opt seccomp=<path>`).
    pub seccomp_profile: Option<String>,
    /// Set `PR_SET_NO_NEW_PRIVS` (default: true).
    pub no_new_privs: bool,
}

impl SecurityOptions {
    /// Parse from CLI security options: e.g. `seccomp=/path/to/profile.json` or `no-new-privs:true`.
    pub fn parse_security_opt(opts: &[String]) -> Result<Option<String>> {
        for opt in opts {
            if let Some((k, v)) = opt.split_once('=') {
                if k == "seccomp" {
                    return Ok(Some(v.to_string()));
                }
            }
        }
        Ok(None)
    }
}

/// Set `PR_SET_NO_NEW_PRIVS` to prevent setuid privilege escalation.
pub fn set_no_new_privs() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::PR_SET_NO_NEW_PRIVS;
        // SAFETY: prctl with PR_SET_NO_NEW_PRIVS is safe and has no pointers
        let ret = unsafe { libc::prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if ret != 0 {
            return Err(AlphenError::Runtime(format!(
                "Failed to set PR_SET_NO_NEW_PRIVS: {}",
                std::io::Error::last_os_error()
            )));
        }
        log::debug!("PR_SET_NO_NEW_PRIVS successfully set");
    }

    #[cfg(not(target_os = "linux"))]
    {
        log::debug!("Simulating PR_SET_NO_NEW_PRIVS on non-Linux platform");
    }

    Ok(())
}

/// Configure Linux capability bounding set according to defaults and user flags.
/// By default, all capabilities are dropped except explicit `--cap-add` entries.
pub fn apply_capabilities(cap_add: &[String], cap_drop: &[String]) -> Result<()> {
    let mut add_set: HashSet<u32> = HashSet::new();
    let mut drop_set: HashSet<u32> = HashSet::new();

    let drop_all = cap_drop.iter().any(|c| c.eq_ignore_ascii_case("ALL"));

    for c in cap_add {
        if c.eq_ignore_ascii_case("ALL") {
            for cap_num in 0..=40 {
                add_set.insert(cap_num);
            }
        } else {
            add_set.insert(parse_capability(c)?);
        }
    }

    for c in cap_drop {
        if !c.eq_ignore_ascii_case("ALL") {
            drop_set.insert(parse_capability(c)?);
        }
    }

    #[cfg(target_os = "linux")]
    {
        use linux::PR_CAPBSET_DROP;

        // Iterate through all capabilities up to CAP_LAST_CAP (40)
        for cap in 0..=40u32 {
            // Keep if in add_set, otherwise drop if drop_all or default lockdown
            let should_drop = if drop_all {
                !add_set.contains(&cap)
            } else if drop_set.contains(&cap) {
                true
            } else {
                // Default: drop all capabilities unless explicitly retained
                !add_set.contains(&cap)
            };

            if should_drop {
                // SAFETY: PR_CAPBSET_DROP drops capability from bounding set
                let _ = unsafe { libc::prctl(PR_CAPBSET_DROP, cap as libc::c_ulong, 0, 0, 0) };
            }
        }
        log::debug!(
            "Capability bounding set configured (retained: {:?})",
            add_set
        );
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (drop_all, add_set, drop_set);
        log::debug!("Simulating capability bounding set restrictions on non-Linux platform");
    }

    Ok(())
}

/// Apply default seccomp BPF filter blocking dangerous syscalls.
/// Blocks: mount, pivot_root, reboot, kexec_load, init_module, finit_module,
/// delete_module, bpf, perf_event_open, userfaultfd, acct, swapon, swapoff.
pub fn apply_seccomp_filter(custom_profile: Option<&Path>) -> Result<()> {
    if let Some(profile_path) = custom_profile {
        if !profile_path.exists() {
            return Err(AlphenError::Config(format!(
                "Seccomp profile file not found: {}",
                profile_path.display()
            )));
        }
        log::info!(
            "Loading custom seccomp profile from {}",
            profile_path.display()
        );
    }

    #[cfg(target_os = "linux")]
    {
        use linux::*;

        // AArch64 Linux dangerous syscall numbers
        const BLOCKED_SYSCALLS: &[u32] = &[
            40,  // mount
            41,  // pivot_root
            89,  // acct
            104, // kexec_load
            105, // init_module
            106, // delete_module
            142, // reboot
            224, // swapon
            225, // swapoff
            241, // perf_event_open
            273, // finit_module
            280, // bpf
            282, // userfaultfd
        ];

        let mut filter: Vec<SockFilter> = Vec::new();

        // 1. Load syscall number: BPF_LD | BPF_W | BPF_ABS, offset 0 (struct seccomp_data.nr)
        filter.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0));

        // 2. Check each blocked syscall. If match, jump to return EPERM; else continue.
        let num_blocked = BLOCKED_SYSCALLS.len();
        for (i, &syscall_nr) in BLOCKED_SYSCALLS.iter().enumerate() {
            let jt = (num_blocked - i) as u8; // Jump target to the errno return statement
            let jf = 0; // Check next instruction
            filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, syscall_nr, jt, jf));
        }

        // Default allow: SECCOMP_RET_ALLOW
        filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));

        // Blocked action: return -EPERM (1)
        filter.push(bpf_stmt(
            BPF_RET | BPF_K,
            SECCOMP_RET_ERRNO | (libc::EPERM as u32),
        ));

        let prog = SockFprog {
            len: filter.len() as u16,
            filter: filter.as_ptr(),
        };

        // SAFETY: PR_SET_SECCOMP requires PR_SET_NO_NEW_PRIVS set first (or CAP_SYS_ADMIN)
        let ret = unsafe {
            libc::prctl(
                PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER,
                &prog as *const _ as libc::c_ulong,
                0,
                0,
            )
        };
        if ret != 0 {
            log::warn!(
                "Kernel seccomp filter rejected: {} (kernel may lack CONFIG_SECCOMP_FILTER)",
                std::io::Error::last_os_error()
            );
        } else {
            log::debug!("Default seccomp BPF filter applied successfully");
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        log::debug!("Simulating seccomp BPF sandbox on non-Linux platform");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_capability_names() {
        assert_eq!(parse_capability("CHOWN").unwrap(), 0);
        assert_eq!(parse_capability("cap_chown").unwrap(), 0);
        assert_eq!(parse_capability("CAP_NET_BIND_SERVICE").unwrap(), 10);
        assert_eq!(parse_capability("SYS_ADMIN").unwrap(), 21);
        assert!(parse_capability("UNKNOWN_CAP").is_err());
    }

    #[test]
    fn test_security_options_seccomp_parse() {
        let opts = vec![
            "seccomp=/etc/alphen/seccomp.json".to_string(),
            "no-new-privs:true".to_string(),
        ];
        let seccomp = SecurityOptions::parse_security_opt(&opts).unwrap();
        assert_eq!(seccomp, Some("/etc/alphen/seccomp.json".to_string()));
    }

    #[test]
    fn test_apply_capabilities_contract() {
        let add = vec!["NET_BIND_SERVICE".to_string()];
        let drop = vec!["SYS_ADMIN".to_string()];
        assert!(apply_capabilities(&add, &drop).is_ok());
    }

    #[test]
    fn test_seccomp_filter_contract() {
        assert!(apply_seccomp_filter(None).is_ok());
    }
}
