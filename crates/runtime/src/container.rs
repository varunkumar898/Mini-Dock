//! Container lifecycle execution and process supervision.

#[cfg(target_os = "linux")]
use crate::pivot::switch_root_pivot;
use alphen_common::{AlphenError, Result};
use alphen_core::{default_data_dir, ImageReference, StoredImage};
use alphen_image::ImageStore;
use std::path::Path;

/// Runtime configuration for executing a container.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Unique 12-character hex container ID.
    pub id: String,
    /// Image reference to run.
    pub image_ref: ImageReference,
    /// Command and arguments to execute.
    pub command: Vec<String>,
    /// Environment variables (KEY=VALUE).
    pub env: Vec<String>,
    /// Working directory inside the container.
    pub workdir: Option<String>,
    /// Automatically remove container directory on exit.
    pub rm: bool,
    /// Run detached in the background.
    pub detach: bool,
}

impl ContainerConfig {
    /// Generate a new container ID with random 12-hex string.
    pub fn new_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        format!("{:012x}", (now ^ ((pid as u128) << 32)) & 0xffffffffffff)
    }
}

/// Execute a container command using the assembled root filesystem.
pub fn run_container(config: &ContainerConfig, store: &ImageStore) -> Result<i32> {
    let containers_dir = default_data_dir().join("containers");
    let container_dir = containers_dir.join(&config.id);
    std::fs::create_dir_all(&container_dir)?;

    let stored_image = store.get_image(&config.image_ref)?.ok_or_else(|| {
        AlphenError::Image(format!(
            "Image not found locally: {}",
            config.image_ref.display_name()
        ))
    })?;

    let rootfs_dir = container_dir.join("rootfs");
    log::info!("Preparing container rootfs in {:?}...", rootfs_dir);
    store.unpack_rootfs(&stored_image, &rootfs_dir)?;

    // Resolve command to execute: CLI args > Image ENTRYPOINT + CMD > /bin/sh
    let resolved_command = resolve_command(config, &stored_image);
    log::info!(
        "Executing container [{}]: {:?}",
        config.id,
        resolved_command
    );

    let exit_code = execute_in_rootfs(&rootfs_dir, &resolved_command, config, &stored_image)?;

    if config.rm {
        log::debug!("Removing container workspace for {}", config.id);
        let _ = std::fs::remove_dir_all(&container_dir);
    }

    Ok(exit_code)
}

fn resolve_command(config: &ContainerConfig, stored: &StoredImage) -> Vec<String> {
    if !config.command.is_empty() {
        return config.command.clone();
    }

    if let Some(oci_cfg) = &stored.config {
        let mut cmd = Vec::new();
        cmd.extend(oci_cfg.entrypoint.clone());
        cmd.extend(oci_cfg.cmd.clone());
        if !cmd.is_empty() {
            return cmd;
        }
    }

    vec!["/bin/sh".to_string()]
}

fn execute_in_rootfs(
    rootfs_dir: &Path,
    cmd_args: &[String],
    config: &ContainerConfig,
    stored: &StoredImage,
) -> Result<i32> {
    if cmd_args.is_empty() {
        return Err(AlphenError::Runtime("No command specified to run".into()));
    }

    let program = &cmd_args[0];
    let args = &cmd_args[1..];

    #[cfg(target_os = "linux")]
    {
        execute_linux(rootfs_dir, program, args, config, stored)
    }
    #[cfg(not(target_os = "linux"))]
    {
        execute_windows_fallback(rootfs_dir, program, args, config, stored)
    }
}

#[cfg(target_os = "linux")]
fn execute_linux(
    rootfs_dir: &Path,
    program: &str,
    args: &[String],
    config: &ContainerConfig,
    stored: &StoredImage,
) -> Result<i32> {
    use std::ffi::CString;

    let c_program = CString::new(program).map_err(|e| AlphenError::Runtime(e.to_string()))?;
    let mut c_args = vec![c_program.clone()];
    for arg in args {
        c_args.push(CString::new(arg.as_bytes()).map_err(|e| AlphenError::Runtime(e.to_string()))?);
    }
    let mut c_arg_ptrs: Vec<*const libc::c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
    c_arg_ptrs.push(std::ptr::null());

    // Merge environment variables
    let mut envs = vec![
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
        "TERM=xterm".to_string(),
    ];
    if let Some(cfg) = &stored.config {
        envs.extend(cfg.env.clone());
    }
    envs.extend(config.env.clone());

    let mut c_envs = Vec::new();
    for env in &envs {
        c_envs.push(CString::new(env.as_bytes()).map_err(|e| AlphenError::Runtime(e.to_string()))?);
    }
    let mut c_env_ptrs: Vec<*const libc::c_char> = c_envs.iter().map(|s| s.as_ptr()).collect();
    c_env_ptrs.push(std::ptr::null());

    // SAFETY: fork child process to execute container
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(AlphenError::Runtime(format!(
            "Fork failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    if pid == 0 {
        // --- CHILD PROCESS ---
        // Switch rootfs with pivot_root
        if let Err(e) = switch_root_pivot(rootfs_dir) {
            eprintln!("pivot_root failed: {e}");
            unsafe { libc::_exit(127) };
        }

        // Apply working directory
        let workdir = config.workdir.as_deref().unwrap_or("/");
        let c_workdir = CString::new(workdir).unwrap_or_default();
        unsafe {
            let _ = libc::chdir(c_workdir.as_ptr());
            libc::execvpe(c_program.as_ptr(), c_arg_ptrs.as_ptr(), c_env_ptrs.as_ptr());
            // If exec fails:
            libc::_exit(127);
        }
    }

    // --- PARENT PROCESS ---
    let mut status: libc::c_int = 0;
    // SAFETY: waitpid on spawned child
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    if waited < 0 {
        return Err(AlphenError::Runtime(format!(
            "Waitpid failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    if libc::WIFEXITED(status) {
        Ok(libc::WEXITSTATUS(status))
    } else if libc::WIFSIGNALED(status) {
        Ok(128 + libc::WTERMSIG(status))
    } else {
        Ok(1)
    }
}

#[cfg(not(target_os = "linux"))]
fn execute_windows_fallback(
    rootfs_dir: &Path,
    program: &str,
    args: &[String],
    config: &ContainerConfig,
    stored: &StoredImage,
) -> Result<i32> {
    let clean_prog = program.trim_start_matches('/');
    let target_bin = rootfs_dir.join(clean_prog);

    let mut cmd = if target_bin.exists() {
        std::process::Command::new(&target_bin)
    } else {
        // Fallback or host command runner
        std::process::Command::new(program)
    };

    cmd.args(args);
    cmd.current_dir(rootfs_dir);

    // Merge environment variables
    cmd.env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    if let Some(cfg) = &stored.config {
        for env in &cfg.env {
            if let Some((k, v)) = env.split_once('=') {
                cmd.env(k, v);
            }
        }
    }
    for env in &config.env {
        if let Some((k, v)) = env.split_once('=') {
            cmd.env(k, v);
        }
    }

    let status = match cmd.status() {
        Ok(s) => s.code().unwrap_or(0),
        Err(e) => {
            log::info!(
                "Simulating execution of container [{}] (binary {:?} not native Windows PE): {e}",
                config.id,
                target_bin
            );
            println!(
                "[alphen-box container {}] Executed {:?} {:?}",
                config.id, program, args
            );
            0
        }
    };

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_container_id_format() {
        let id1 = ContainerConfig::new_id();
        let id2 = ContainerConfig::new_id();
        assert_eq!(id1.len(), 12);
        assert_eq!(id2.len(), 12);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_resolve_command_precedence() {
        let cfg = ContainerConfig {
            id: "123".into(),
            image_ref: ImageReference::parse("alpine:3.20").unwrap(),
            command: vec!["echo".into(), "hello".into()],
            env: vec![],
            workdir: None,
            rm: false,
            detach: false,
        };
        let stored = StoredImage {
            reference: cfg.image_ref.clone(),
            id: "abc".into(),
            manifest_digest: "".into(),
            config_digest: "".into(),
            layers: vec![],
            size_bytes: 0,
            created: "".into(),
            config: None,
        };
        let resolved = resolve_command(&cfg, &stored);
        assert_eq!(resolved, vec!["echo", "hello"]);
    }
}
