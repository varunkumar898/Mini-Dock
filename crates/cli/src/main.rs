#![forbid(unsafe_code)]

//! `alphen-box`: Ultra-lightweight container runtime for Alphen OS (ARM64).

use alphen_common::{AlphenError, Result};
use alphen_core::ImageReference;
use alphen_image::ImageStore;
use alphen_runtime::ContainerConfig;
use clap::{Args, Parser, Subcommand};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const TARGET_TRIPLE: &str = "aarch64-unknown-linux-musl";

/// Minimal, high-performance container runtime for Alphen OS
#[derive(Parser, Debug)]
#[command(
    name = "alphen-box",
    about = "Lightweight container runtime for Alphen OS (ARM64)",
    version = VERSION
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show version and build environment
    Version,

    /// List local container images
    Images,

    /// Pull an image from an OCI/Docker registry
    Pull {
        /// Image name (e.g. alpine:3.20 or ghcr.io/org/repo:tag)
        image: String,
    },

    /// Run a command in a new container
    Run(Box<RunArgs>),

    /// List containers
    Ps {
        /// Show all containers (default shows just running)
        #[arg(short = 'a', long = "all")]
        all: bool,
    },

    /// Fetch the logs of a container
    Logs {
        /// Follow log output
        #[arg(short = 'f', long = "follow")]
        follow: bool,
        /// Container name or ID
        container: String,
    },

    /// Run a command in an existing container
    Exec {
        /// Keep STDIN open even if not attached
        #[arg(short = 'i', long = "interactive")]
        interactive: bool,
        /// Allocate a pseudo-TTY
        #[arg(short = 't', long = "tty")]
        tty: bool,
        /// Container name or ID
        container: String,
        /// Command to execute
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },

    /// Stop one or more running containers
    Stop {
        /// Container name or ID
        container: String,
    },

    /// Start one or more stopped containers
    Start {
        /// Container name or ID
        container: String,
    },

    /// Remove one or more containers
    Rm {
        /// Force the removal of a running container
        #[arg(short = 'f', long = "force")]
        force: bool,
        /// Container name or ID
        container: String,
    },

    /// Return low-level information on container or image
    Inspect {
        /// Container or image name/ID
        target: String,
    },

    /// Remove one or more images
    Rmi {
        /// Image name or ID
        image: String,
    },

    /// Clean up unreferenced image blobs and layers
    Gc,
}

/// Arguments for `alphen-box run`
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Run container in background and print container ID
    #[arg(short = 'd', long = "detach")]
    pub detach: bool,

    /// Keep STDIN open even if not attached
    #[arg(short = 'i', long = "interactive")]
    pub interactive: bool,

    /// Allocate a pseudo-TTY
    #[arg(short = 't', long = "tty")]
    pub tty: bool,

    /// Assign a name to the container
    #[arg(long = "name")]
    pub name: Option<String>,

    /// Set environment variables (-e KEY=VALUE)
    #[arg(short = 'e', long = "env")]
    pub env: Vec<String>,

    /// Bind mount a volume (-v /host:/container[:ro])
    #[arg(short = 'v', long = "volume")]
    pub volumes: Vec<String>,

    /// Publish a container's port(s) to the host (-p 8080:80)
    #[arg(short = 'p', long = "publish")]
    pub ports: Vec<String>,

    /// Connect a container to a network (bridge, host, none)
    #[arg(long = "network", default_value = "bridge")]
    pub network: String,

    /// Memory limit (e.g. 64m, 1g)
    #[arg(long = "memory")]
    pub memory: Option<String>,

    /// Swap memory limit (e.g. 128m)
    #[arg(long = "memory-swap")]
    pub memory_swap: Option<String>,

    /// Number of CPUs (e.g. 1.5)
    #[arg(long = "cpus")]
    pub cpus: Option<f32>,

    /// CPU shares (relative weight, e.g. 1024)
    #[arg(long = "cpu-shares")]
    pub cpu_shares: Option<u64>,

    /// Tune container pids limit
    #[arg(long = "pids-limit")]
    pub pids_limit: Option<u32>,

    /// Limit read rate (bytes per second) from a device
    #[arg(long = "device-read-bps")]
    pub device_read_bps: Option<String>,

    /// Limit write rate (bytes per second) to a device
    #[arg(long = "device-write-bps")]
    pub device_write_bps: Option<String>,

    /// Automatically remove the container when it exits
    #[arg(long = "rm")]
    pub rm: bool,

    /// Mount the container's root filesystem as read only
    #[arg(long = "read-only")]
    pub read_only: bool,

    /// Mount a tmpfs directory (--tmpfs /path)
    #[arg(long = "tmpfs")]
    pub tmpfs: Vec<String>,

    /// Container host name
    #[arg(long = "hostname")]
    pub hostname: Option<String>,

    /// Username or UID
    #[arg(short = 'u', long = "user")]
    pub user: Option<String>,

    /// Working directory inside the container
    #[arg(short = 'w', long = "workdir")]
    pub workdir: Option<String>,

    /// Overwrite the default ENTRYPOINT of the image
    #[arg(long = "entrypoint")]
    pub entrypoint: Option<String>,

    /// Image to run
    pub image: String,

    /// Command and arguments to execute
    #[arg(trailing_var_arg = true)]
    pub command: Vec<String>,
}

fn main() -> ExitCode {
    alphen_common::init_logger();

    let cli = Cli::parse();

    if let Err(err) = run_app(cli) {
        eprintln!("alphen-box error: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run_app(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Version => {
            println!("alphen-box version {VERSION}");
            println!("Target: {TARGET_TRIPLE}");
            println!("Alphen OS ARM64 Container Runtime");
            println!("Engine: Minimal daemonless runner (OverlayFS + cgroups v2)");

            // Check system requirements if running on host
            match alphen_runtime::verify_system_requirements() {
                Ok(caps) => {
                    println!(
                        "Kernel: {} (supported: {})",
                        caps.kernel_version, caps.kernel_supported
                    );
                    println!(
                        "Features: cgroups_v2={} userns={} overlayfs={} seccomp={}",
                        caps.cgroups_v2, caps.user_namespaces, caps.overlayfs, caps.seccomp
                    );
                }
                Err(e) => {
                    eprintln!("Warning: Host system requirement check: {e}");
                }
            }
            Ok(())
        }

        Commands::Images => {
            let store = ImageStore::new_default();
            let images = store.list_images()?;

            println!(
                "{:<30} {:<15} {:<15} {:<12} {:<10}",
                "REPOSITORY", "TAG", "IMAGE ID", "CREATED", "SIZE"
            );
            if images.is_empty() {
                // Empty output like docker
            } else {
                for img in images {
                    println!(
                        "{:<30} {:<15} {:<15} {:<12} {:<10}",
                        img.repository,
                        img.tag,
                        img.id,
                        img.created,
                        img.formatted_size()
                    );
                }
            }
            Ok(())
        }

        Commands::Pull { image } => {
            let image_ref = ImageReference::parse(&image)?;
            println!("Pulling from {}...", image_ref.display_name());
            let store = ImageStore::new_default();
            let stored = store.pull(&image_ref)?;
            println!("Status: Downloaded newer image for {image}");
            println!("Digest: {}", stored.manifest_digest);
            println!("ID: {}", stored.id);
            Ok(())
        }

        Commands::Run(args) => {
            let image_ref = ImageReference::parse(&args.image)?;
            let store = ImageStore::new_default();

            // Pull image if not already cached
            if store.get_image(&image_ref)?.is_none() {
                println!(
                    "Unable to find image '{}' locally",
                    image_ref.display_name()
                );
                println!("Pulling from {}...", image_ref.display_name());
                store.pull(&image_ref)?;
            }

            let container_id = args.name.clone().unwrap_or_else(ContainerConfig::new_id);

            let mut volume_mounts = Vec::new();
            for v in &args.volumes {
                volume_mounts.push(alphen_runtime::VolumeMount::parse(v)?);
            }

            let memory_bytes = match &args.memory {
                Some(m) => Some(alphen_runtime::parse_memory_str(m)?),
                None => None,
            };
            let memory_swap_bytes = match &args.memory_swap {
                Some(m) => Some(alphen_runtime::parse_memory_str(m)?),
                None => None,
            };

            let limits = alphen_runtime::ResourceLimits {
                memory_bytes,
                memory_swap_bytes,
                cpus: args.cpus,
                cpu_shares: args.cpu_shares,
                pids_limit: args.pids_limit,
                device_read_bps: args.device_read_bps.clone(),
                device_write_bps: args.device_write_bps.clone(),
            };

            let config = ContainerConfig {
                id: container_id,
                image_ref,
                command: args.command.clone(),
                env: args.env.clone(),
                workdir: args.workdir.clone(),
                volumes: volume_mounts,
                tmpfs: args.tmpfs.clone(),
                limits,
                read_only: args.read_only,
                hostname: args.hostname.clone(),
                rm: args.rm,
                detach: args.detach,
            };

            let exit_code = alphen_runtime::run_container(&config, &store)?;
            if exit_code != 0 {
                return Err(AlphenError::Runtime(format!(
                    "Container exited with status {exit_code}"
                )));
            }
            Ok(())
        }

        Commands::Ps { all } => {
            let states = alphen_runtime::list_states(all)?;
            println!(
                "{:<15} {:<20} {:<22} {:<15} {:<18} {:<15}",
                "CONTAINER ID", "IMAGE", "COMMAND", "CREATED", "STATUS", "NAMES"
            );
            for s in states {
                let short_id = if s.id.len() >= 12 { &s.id[..12] } else { &s.id };
                println!(
                    "{:<15} {:<20} {:<22} {:<15} {:<18} {:<15}",
                    short_id,
                    s.image,
                    s.formatted_command(),
                    s.created,
                    s.formatted_status(),
                    s.name
                );
            }
            Ok(())
        }

        Commands::Logs { follow, container } => {
            alphen_runtime::read_container_logs(&container, follow)
        }

        Commands::Exec {
            interactive,
            tty,
            container,
            command,
        } => {
            let exit_code = alphen_runtime::exec_container(&container, &command, interactive, tty)?;
            if exit_code != 0 {
                return Err(AlphenError::Runtime(format!(
                    "Exec exited with status {exit_code}"
                )));
            }
            Ok(())
        }

        Commands::Stop { container } => alphen_runtime::stop_container(&container, 10),

        Commands::Start { container } => {
            let state = alphen_runtime::get_state(&container)?
                .ok_or_else(|| AlphenError::Runtime(format!("No such container: '{container}'")))?;
            let image_ref = ImageReference::parse(&state.image)?;
            let store = ImageStore::new_default();
            let config = ContainerConfig {
                id: state.id.clone(),
                image_ref,
                command: state.command.clone(),
                env: vec![],
                workdir: None,
                volumes: vec![],
                tmpfs: vec![],
                limits: alphen_runtime::ResourceLimits::default(),
                read_only: false,
                hostname: Some(state.name.clone()),
                rm: false,
                detach: true,
            };
            alphen_runtime::run_container(&config, &store)?;
            println!("{}", state.id);
            Ok(())
        }

        Commands::Rm { force, container } => alphen_runtime::rm_container(&container, force),

        Commands::Inspect { target } => {
            let store = ImageStore::new_default();
            // 1. Try matching container state
            if let Ok(Some(state)) = alphen_runtime::get_state(&target) {
                let pretty = serde_json::to_string_pretty(&state)
                    .map_err(|e| AlphenError::Config(format!("JSON error: {e}")))?;
                println!("{pretty}");
                return Ok(());
            }

            // 2. Try matching image
            if let Ok(image_ref) = ImageReference::parse(&target) {
                if let Ok(Some(stored)) = store.get_image(&image_ref) {
                    let pretty = serde_json::to_string_pretty(&stored)
                        .map_err(|e| AlphenError::Config(format!("JSON error: {e}")))?;
                    println!("{pretty}");
                    return Ok(());
                }
            }

            println!("{{ \"target\": \"{target}\", \"status\": \"not_found\" }}");
            Ok(())
        }

        Commands::Rmi { image } => {
            let image_ref = ImageReference::parse(&image)?;
            let store = ImageStore::new_default();
            if store.remove_image(&image_ref)? {
                println!("Untagged: {image}");
            } else {
                eprintln!("Error: No such image: {image}");
            }
            Ok(())
        }

        Commands::Gc => {
            let store = ImageStore::new_default();
            let report = store.gc()?;
            println!(
                "Total reclaimed space: {} B across {} orphaned blob(s)",
                report.bytes_freed, report.blobs_removed
            );
            Ok(())
        }
    }
}
