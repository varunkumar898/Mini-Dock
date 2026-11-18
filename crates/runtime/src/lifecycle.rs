//! Container lifecycle management: stop, start, rm, and exec.

#[cfg(target_os = "linux")]
use crate::state::is_process_alive;
use crate::state::{delete_state, get_state, save_state};
use alphen_common::{AlphenError, Result};
use alphen_core::ContainerStatus;

/// Stop a running container by sending SIGTERM, followed by SIGKILL if timeout expires.
pub fn stop_container(id_or_name: &str, timeout_secs: u64) -> Result<()> {
    let mut state = get_state(id_or_name)?
        .ok_or_else(|| AlphenError::Runtime(format!("No such container: '{id_or_name}'")))?;

    if state.status != ContainerStatus::Running {
        log::info!("Container {} is already stopped", state.id);
        return Ok(());
    }

    if let Some(pid) = state.pid {
        #[cfg(target_os = "linux")]
        {
            log::info!("Sending SIGTERM to container PID {pid}...");
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }

            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(timeout_secs);
            let mut stopped = false;

            while start.elapsed() < timeout {
                if !is_process_alive(pid) {
                    stopped = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            if !stopped {
                log::warn!(
                    "Container PID {pid} did not exit after {timeout_secs}s, sending SIGKILL..."
                );
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (pid, timeout_secs);
        }
    }

    state.status = ContainerStatus::Exited;
    state.exit_code = Some(137); // Standard SIGKILL exit code
    save_state(&state)?;
    println!("{}", state.id);
    Ok(())
}

/// Remove a container from local metadata and filesystem storage.
pub fn rm_container(id_or_name: &str, force: bool) -> Result<()> {
    let state = get_state(id_or_name)?
        .ok_or_else(|| AlphenError::Runtime(format!("No such container: '{id_or_name}'")))?;

    if state.status == ContainerStatus::Running {
        if force {
            stop_container(&state.id, 2)?;
        } else {
            return Err(AlphenError::Runtime(format!(
                "You cannot remove a running container {}. Stop the container before removing or use -f",
                state.id
            )));
        }
    }

    delete_state(&state.id)?;
    println!("{}", state.id);
    Ok(())
}

/// Execute a command in an existing running container.
pub fn exec_container(
    id_or_name: &str,
    command: &[String],
    interactive: bool,
    tty: bool,
) -> Result<i32> {
    let state = get_state(id_or_name)?
        .ok_or_else(|| AlphenError::Runtime(format!("No such container: '{id_or_name}'")))?;

    if state.status != ContainerStatus::Running {
        return Err(AlphenError::Runtime(format!(
            "Container {} is not running",
            state.id
        )));
    }

    let _pid = state
        .pid
        .ok_or_else(|| AlphenError::Runtime(format!("Container {} has no active PID", state.id)))?;

    #[cfg(target_os = "linux")]
    {
        exec_linux(_pid, command, interactive, tty)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interactive, tty);
        println!("[alphen-box exec {}] Executing {:?}", state.id, command);
        Ok(0)
    }
}

#[cfg(target_os = "linux")]
fn exec_linux(target_pid: u32, command: &[String], _interactive: bool, _tty: bool) -> Result<i32> {
    use std::ffi::CString;
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    // Join target container namespaces via setns(2) (man 2 setns)
    let ns_types = ["mnt", "pid", "net", "ipc", "uts"];
    for ns in ns_types {
        let ns_path = format!("/proc/{target_pid}/ns/{ns}");
        if let Ok(f) = File::open(&ns_path) {
            let fd = f.as_raw_fd();
            // SAFETY: entering container namespace via setns
            unsafe {
                libc::setns(fd, 0);
            }
        }
    }

    if command.is_empty() {
        return Err(AlphenError::Runtime("No command provided to exec".into()));
    }

    let c_program = CString::new(command[0].as_bytes()).unwrap();
    let mut c_args = vec![c_program.clone()];
    for a in &command[1..] {
        c_args.push(CString::new(a.as_bytes()).unwrap());
    }
    let mut c_arg_ptrs: Vec<*const libc::c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
    c_arg_ptrs.push(std::ptr::null());

    let env = [
        CString::new("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin").unwrap(),
    ];
    let env_ptrs = [env[0].as_ptr(), std::ptr::null()];

    // SAFETY: fork child to exec in target namespaces
    let child = unsafe { libc::fork() };
    if child == 0 {
        unsafe {
            let slash = CString::new("/").unwrap();
            let _ = libc::chroot(slash.as_ptr());
            let _ = libc::chdir(slash.as_ptr());
            libc::execvpe(c_program.as_ptr(), c_arg_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::_exit(127);
        }
    }

    let mut status = 0;
    // SAFETY: wait on exec child
    unsafe {
        libc::waitpid(child, &mut status, 0);
    }

    if libc::WIFEXITED(status) {
        Ok(libc::WEXITSTATUS(status))
    } else {
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rm_non_existent_container_error() {
        let res = rm_container("non_existent_9999", false);
        assert!(res.is_err());
    }
}
