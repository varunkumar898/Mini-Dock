//! `docker` compatibility wrapper for `alphen-box`.
//!
//! Provides drop-in compatibility for Docker scripts and CLI habits.
//! Translates standard Docker invocations into `alphen-box` commands,
//! and prints informative warnings for unsupported flags instead of failing silently.

use std::process::{Command, ExitCode};

const UNSUPPORTED_FLAGS: &[&str] = &[
    "--restart",
    "--gpus",
    "--platform",
    "--label",
    "-l",
    "--add-host",
    "--init",
    "--log-opt",
    "--ulimit",
    "--security-opt=apparmor",
    "--cgroupns",
    "--ipc",
    "--health-cmd",
];

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();

    if raw_args.is_empty() {
        eprintln!("Usage: docker <command> [options]");
        eprintln!("(Compatibility alias for alphen-box)");
        return ExitCode::from(1);
    }

    let mut filtered_args = Vec::new();
    let mut skip_next = false;

    for (i, arg) in raw_args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        let is_unsupported = UNSUPPORTED_FLAGS
            .iter()
            .any(|unsupported| arg == *unsupported || arg.starts_with(&format!("{unsupported}=")));

        if is_unsupported {
            eprintln!("alphen-box warning: flag '{arg}' is not supported in alphen-box v1 and was ignored");
            // If the flag takes a value separated by space, skip the next arg
            if !arg.contains('=') && i + 1 < raw_args.len() && !raw_args[i + 1].starts_with('-') {
                skip_next = true;
            }
            continue;
        }

        filtered_args.push(arg.clone());
    }

    // Locate alphen-box binary next to this executable or in PATH
    let alphen_box_bin = match std::env::current_exe() {
        Ok(exe_path) => {
            let candidate = if cfg!(target_os = "windows") {
                exe_path.with_file_name("alphen-box.exe")
            } else {
                exe_path.with_file_name("alphen-box")
            };
            if candidate.exists() {
                candidate.to_string_lossy().to_string()
            } else {
                "alphen-box".to_string()
            }
        }
        Err(_) => "alphen-box".to_string(),
    };

    let status = match Command::new(&alphen_box_bin).args(&filtered_args).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("alphen-box error: failed to execute '{alphen_box_bin}': {e}");
            1
        }
    };

    ExitCode::from(status as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_flags_list() {
        assert!(UNSUPPORTED_FLAGS.contains(&"--restart"));
        assert!(UNSUPPORTED_FLAGS.contains(&"--gpus"));
    }
}
