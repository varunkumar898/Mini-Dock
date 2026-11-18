//! Container volatile state (`/run/alphen-box/`) and persistent metadata storage.

use alphen_common::{AlphenError, Result};
use alphen_core::{default_containers_dir, default_run_dir, ContainerState, ContainerStatus};
#[cfg(target_os = "linux")]
use std::fs::File;
use std::path::Path;

/// Save container runtime state atomically to volatile `/run/alphen-box/<id>.json`
/// and persistent `/var/lib/alphen-box/containers/<id>/state.json`.
pub fn save_state(state: &ContainerState) -> Result<()> {
    let run_dir = default_run_dir();
    let persistent_dir = default_containers_dir().join(&state.id);
    std::fs::create_dir_all(&run_dir)?;
    std::fs::create_dir_all(&persistent_dir)?;

    let state_json = serde_json::to_string_pretty(state)
        .map_err(|e| AlphenError::Runtime(format!("Failed to serialize container state: {e}")))?;

    // 1. Write volatile state
    let volatile_path = run_dir.join(format!("{}.json", state.id));
    atomic_write(&volatile_path, &state_json)?;

    // 2. Write persistent state
    let persistent_path = persistent_dir.join("state.json");
    atomic_write(&persistent_path, &state_json)?;

    Ok(())
}

/// Retrieve container state by ID prefix or exact name.
pub fn get_state(id_or_name: &str) -> Result<Option<ContainerState>> {
    let all = list_states(true)?;
    for s in all {
        if s.id.starts_with(id_or_name) || s.name == id_or_name {
            return Ok(Some(s));
        }
    }
    Ok(None)
}

/// List container states, filtering running only if `all` is false.
pub fn list_states(all: bool) -> Result<Vec<ContainerState>> {
    let run_dir = default_run_dir();
    let persistent_base = default_containers_dir();
    let mut states = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // 1. Read volatile states
    if run_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&run_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(mut state) = serde_json::from_str::<ContainerState>(&content) {
                            // Check if process is still alive on host
                            if state.status == ContainerStatus::Running {
                                if let Some(pid) = state.pid {
                                    if !is_process_alive(pid) {
                                        state.status = ContainerStatus::Exited;
                                        let _ = save_state(&state);
                                    }
                                }
                            }
                            seen_ids.insert(state.id.clone());
                            if all || state.status == ContainerStatus::Running {
                                states.push(state);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Reconcile persistent states after host reboot
    if persistent_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&persistent_base) {
            for entry in entries.flatten() {
                let id_dir = entry.path();
                if id_dir.is_dir() {
                    let state_file = id_dir.join("state.json");
                    if state_file.exists() {
                        if let Ok(content) = std::fs::read_to_string(&state_file) {
                            if let Ok(mut state) = serde_json::from_str::<ContainerState>(&content)
                            {
                                if !seen_ids.contains(&state.id) {
                                    // Reboot occurred; process died
                                    if state.status == ContainerStatus::Running {
                                        state.status = ContainerStatus::Exited;
                                    }
                                    if all || state.status == ContainerStatus::Running {
                                        states.push(state);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort newest first
    states.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(states)
}

/// Delete container state files.
pub fn delete_state(id: &str) -> Result<()> {
    let run_file = default_run_dir().join(format!("{id}.json"));
    if run_file.exists() {
        let _ = std::fs::remove_file(run_file);
    }
    let container_dir = default_containers_dir().join(id);
    if container_dir.exists() {
        let _ = std::fs::remove_dir_all(container_dir);
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().unwrap();
    let tmp = parent.join(format!(
        ".tmp-{}",
        path.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&tmp, content)?;

    #[cfg(target_os = "linux")]
    {
        // On Linux, use flock to prevent concurrent write collisions (man 2 flock)
        use std::os::unix::io::AsRawFd;
        if let Ok(f) = File::open(&tmp) {
            let fd = f.as_raw_fd();
            unsafe {
                libc::flock(fd, libc::LOCK_EX);
            }
        }
    }

    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Check if a PID is alive on the host.
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // kill(pid, 0) checks if process exists without sending a signal (man 2 kill)
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_get_state() {
        let temp = std::env::temp_dir().join("alphen_test_state");
        let _ = std::fs::remove_dir_all(&temp);
        std::env::set_var("ALPHEN_DATA_DIR", temp.join("data"));
        std::env::set_var("ALPHEN_RUN_DIR", temp.join("run"));

        let state = ContainerState {
            id: "aabbcc112233".into(),
            name: "test-cont".into(),
            image: "alpine:3.20".into(),
            command: vec!["/bin/sh".into()],
            status: ContainerStatus::Running,
            pid: Some(12345),
            exit_code: None,
            created: "2026-11-18".into(),
            started_at: Some("2026-11-18".into()),
            finished_at: None,
        };

        save_state(&state).unwrap();

        let fetched = get_state("test-cont").unwrap().unwrap();
        assert_eq!(fetched.id, "aabbcc112233");
        assert_eq!(fetched.name, "test-cont");

        let fetched_by_id = get_state("aabbcc").unwrap().unwrap();
        assert_eq!(fetched_by_id.name, "test-cont");

        delete_state("aabbcc112233").unwrap();
        assert!(get_state("test-cont").unwrap().is_none());

        let _ = std::fs::remove_dir_all(&temp);
        std::env::remove_var("ALPHEN_DATA_DIR");
        std::env::remove_var("ALPHEN_RUN_DIR");
    }
}
