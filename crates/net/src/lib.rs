//! `alphen-net`: Low-overhead direct netlink and nftables container networking.

pub mod dns;
pub mod ipam;
pub mod netlink;
pub mod nftables;

use alphen_common::{AlphenError, Result};
use ipam::{IpamManager, DEFAULT_BRIDGE_GATEWAY, DEFAULT_BRIDGE_SUBNET};
use netlink::BRIDGE_NAME;
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;

/// Network driver mode for `alphen-box` containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Isolated network namespace without loopback or external interfaces.
    None,
    /// Share the host network namespace directly.
    Host,
    /// Connect to standard Linux bridge (`alphen0`) via veth pair.
    Bridge,
}

impl FromStr for NetworkMode {
    type Err = AlphenError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(NetworkMode::None),
            "host" => Ok(NetworkMode::Host),
            "bridge" => Ok(NetworkMode::Bridge),
            other => Err(AlphenError::Config(format!(
                "Invalid network mode '{other}': expected none, host, or bridge"
            ))),
        }
    }
}

/// Port mapping specification: `HOST:CONTAINER[/PROTOCOL]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortMapping {
    /// Host port number.
    pub host_port: u16,
    /// Container port number.
    pub container_port: u16,
    /// IP protocol (default: TCP).
    pub protocol: PortProtocol,
}

/// Supported transport protocol for port forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortProtocol {
    /// Transmission Control Protocol.
    Tcp,
    /// User Datagram Protocol.
    Udp,
}

impl FromStr for PortMapping {
    type Err = AlphenError;

    fn from_str(s: &str) -> Result<Self> {
        let (ports, proto) = match s.split_once('/') {
            Some((p, pr)) => match pr.to_ascii_lowercase().as_str() {
                "tcp" => (p, PortProtocol::Tcp),
                "udp" => (p, PortProtocol::Udp),
                _ => {
                    return Err(AlphenError::Config(format!(
                        "Invalid protocol '{pr}' in port mapping '{s}', expected tcp or udp"
                    )))
                }
            },
            None => (s, PortProtocol::Tcp),
        };

        let (host_str, container_str) = ports.split_once(':').ok_or_else(|| {
            AlphenError::Config(format!(
                "Invalid port mapping '{s}': expected HOST:CONTAINER[/tcp|udp]"
            ))
        })?;

        let host_port = host_str
            .parse::<u16>()
            .map_err(|_| AlphenError::Config(format!("Invalid host port '{host_str}' in '{s}'")))?;
        let container_port = container_str.parse::<u16>().map_err(|_| {
            AlphenError::Config(format!("Invalid container port '{container_str}' in '{s}'"))
        })?;

        Ok(PortMapping {
            host_port,
            container_port,
            protocol: proto,
        })
    }
}

/// Generate deterministic Linux network interface name for host veth (max 15 chars).
pub fn host_veth_name(container_id: &str) -> String {
    let short_id = if container_id.len() >= 8 {
        &container_id[..8]
    } else {
        container_id
    };
    format!("veth_{short_id}")
}

/// Generate deterministic peer veth name before moving into container netns.
pub fn peer_veth_name(container_id: &str) -> String {
    let short_id = if container_id.len() >= 8 {
        &container_id[..8]
    } else {
        container_id
    };
    format!("ceth_{short_id}")
}

/// Provision and attach container network based on chosen mode.
pub fn setup_container_network(
    container_id: &str,
    container_pid: u32,
    mode: NetworkMode,
    ports: &[PortMapping],
    dns_servers: &[String],
    rootfs: &Path,
) -> Result<Option<Ipv4Addr>> {
    match mode {
        NetworkMode::Host => {
            log::info!("Container {container_id} using host network namespace");
            Ok(None)
        }
        NetworkMode::None => {
            log::info!("Container {container_id} running in isolated loopback-only network");
            dns::write_resolv_conf(rootfs, dns_servers)?;
            Ok(None)
        }
        NetworkMode::Bridge => {
            log::info!(
                "Configuring bridge network for container {container_id} (PID {container_pid})"
            );

            // 1. Write DNS /etc/resolv.conf
            dns::write_resolv_conf(rootfs, dns_servers)?;

            // 2. Ensure bridge alphen0 exists
            let bridge_ifindex = netlink::ensure_bridge(BRIDGE_NAME, DEFAULT_BRIDGE_GATEWAY)?;

            // 3. Setup nftables NAT table
            nftables::setup_nat_table(DEFAULT_BRIDGE_SUBNET, BRIDGE_NAME)?;

            // 4. Allocate IP address from IPAM
            let ipam = IpamManager::new_default();
            let container_ip = ipam.allocate_ip(container_id)?;

            // 5. Create veth pair
            let h_veth = host_veth_name(container_id);
            let p_veth = peer_veth_name(container_id);
            netlink::create_veth_pair(&h_veth, &p_veth)?;

            // 6. Attach host veth to bridge and bring UP
            netlink::set_link_master(&h_veth, bridge_ifindex)?;
            netlink::set_link_up(&h_veth)?;

            // 7. Move peer veth into container netns
            if container_pid > 0 {
                netlink::set_link_netns_pid(&p_veth, container_pid)?;
            }

            // 8. Configure port forwarding rules
            for mapping in ports {
                nftables::add_port_forward(mapping, container_ip)?;
            }

            Ok(Some(container_ip))
        }
    }
}

/// Clean up container veth interface, IPAM lease, and port forwarding rules.
pub fn teardown_container_network(
    container_id: &str,
    mode: NetworkMode,
    ports: &[PortMapping],
    container_ip: Option<Ipv4Addr>,
) -> Result<()> {
    if mode != NetworkMode::Bridge {
        return Ok(());
    }

    log::info!("Tearing down network for container {container_id}");

    // 1. Remove host veth link (kernel deletes peer automatically)
    let h_veth = host_veth_name(container_id);
    let _ = netlink::delete_link(&h_veth);

    // 2. Remove port forwarding rules
    if let Some(ip) = container_ip {
        for mapping in ports {
            let _ = nftables::remove_port_forward(mapping, ip);
        }
    }

    // 3. Release IP address in IPAM
    let ipam = IpamManager::new_default();
    let _ = ipam.release_ip(container_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_mode_parse() {
        assert_eq!("none".parse::<NetworkMode>().unwrap(), NetworkMode::None);
        assert_eq!("host".parse::<NetworkMode>().unwrap(), NetworkMode::Host);
        assert_eq!(
            "bridge".parse::<NetworkMode>().unwrap(),
            NetworkMode::Bridge
        );
        assert!("invalid".parse::<NetworkMode>().is_err());
    }

    #[test]
    fn test_port_mapping_parse() {
        let p1 = "8080:80".parse::<PortMapping>().unwrap();
        assert_eq!(p1.host_port, 8080);
        assert_eq!(p1.container_port, 80);
        assert_eq!(p1.protocol, PortProtocol::Tcp);

        let p2 = "5353:53/udp".parse::<PortMapping>().unwrap();
        assert_eq!(p2.host_port, 5353);
        assert_eq!(p2.container_port, 53);
        assert_eq!(p2.protocol, PortProtocol::Udp);
    }

    #[test]
    fn test_veth_name_length() {
        let id = "abcdef0123456789abcdef";
        let h_name = host_veth_name(id);
        assert!(h_name.len() <= 15);
        assert_eq!(h_name, "veth_abcdef01");
    }
}
