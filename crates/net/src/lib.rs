//! `alphen-net`: Low-overhead direct netlink and nftables container networking.

use alphen_common::{AlphenError, Result};
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
}
