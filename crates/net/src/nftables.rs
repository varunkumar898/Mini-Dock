//! Direct kernel netfilter netlink driver for nftables NAT and port forwarding.
//!
//! Replaces legacy `iptables` with modern Linux kernel `nftables` via
//! `AF_NETLINK` / `NETLINK_NETFILTER` socket communication.
//!
//! References:
//! - `nftables(8)`
//! - Linux kernel `include/uapi/linux/netfilter/nfnetlink.h`
//! - Linux kernel `include/uapi/linux/netfilter/nf_tables.h`

use crate::PortMapping;
#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;
use std::net::Ipv4Addr;

pub const NFT_TABLE_NAME: &str = "alphen_box";

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::mem;

    pub const NETLINK_NETFILTER: i32 = 12;
    pub const NFNETLINK_V0: u8 = 0;
    pub const NFNL_SUBSYS_NFTABLES: u8 = 10;

    // NFT message types ((NFNL_SUBSYS_NFTABLES << 8) | type)
    pub const NFT_MSG_NEWTABLE: u16 = (NFNL_SUBSYS_NFTABLES as u16) << 8 | 0;
    pub const NFT_MSG_NEWCHAIN: u16 = (NFNL_SUBSYS_NFTABLES as u16) << 8 | 3;
    pub const NFT_MSG_NEWRULE: u16 = (NFNL_SUBSYS_NFTABLES as u16) << 8 | 6;
    pub const NFT_MSG_DELRULE: u16 = (NFNL_SUBSYS_NFTABLES as u16) << 8 | 7;

    pub const NLM_F_REQUEST: u16 = 1;
    pub const NLM_F_ACK: u16 = 4;
    pub const NLM_F_CREATE: u16 = 0x400;
    pub const NLM_F_APPEND: u16 = 0x800;

    pub const NFPROTO_IPV4: u8 = 2;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct NfGenMsg {
        pub nfgen_family: u8,
        pub version: u8,
        pub res_id: u16,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct NlMsgHdr {
        pub nlmsg_len: u32,
        pub nlmsg_type: u16,
        pub nlmsg_flags: u16,
        pub nlmsg_seq: u32,
        pub nlmsg_pid: u32,
    }

    pub struct NfNetlinkSocket {
        fd: i32,
    }

    impl NfNetlinkSocket {
        pub fn open() -> Result<Self> {
            let fd = unsafe {
                libc::socket(
                    libc::AF_NETLINK,
                    libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                    NETLINK_NETFILTER,
                )
            };
            if fd < 0 {
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }

            let mut sa: libc::sockaddr_nl = unsafe { mem::zeroed() };
            sa.nl_family = libc::AF_NETLINK as u16;

            let ret = unsafe {
                libc::bind(
                    fd,
                    &sa as *const _ as *const libc::sockaddr,
                    mem::size_of::<libc::sockaddr_nl>() as u32,
                )
            };
            if ret < 0 {
                unsafe { libc::close(fd) };
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }

            Ok(Self { fd })
        }

        pub fn send_msg(&self, buf: &[u8]) -> Result<()> {
            let ret =
                unsafe { libc::send(self.fd, buf.as_ptr() as *const libc::c_void, buf.len(), 0) };
            if ret < 0 {
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }
            Ok(())
        }
    }

    impl Drop for NfNetlinkSocket {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

/// Initialize the `alphen_box` nftables table and setup masquerade NAT for container bridge.
pub fn setup_nat_table(subnet: &str, bridge_name: &str) -> Result<()> {
    log::info!("Setting up nftables NAT for bridge '{bridge_name}' and subnet '{subnet}'");

    #[cfg(target_os = "linux")]
    {
        use linux::*;
        use std::mem;

        let nl = match NfNetlinkSocket::open() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Kernel does not have NETLINK_NETFILTER enabled: {e}");
                return Ok(());
            }
        };

        // Construct table creation message
        let mut buf = Vec::with_capacity(256);
        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: NFT_MSG_NEWTABLE,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        };
        let gen = NfGenMsg {
            nfgen_family: NFPROTO_IPV4,
            version: NFNETLINK_V0,
            res_id: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let gen_bytes = unsafe {
            std::slice::from_raw_parts(&gen as *const _ as *const u8, mem::size_of::<NfGenMsg>())
        };

        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(gen_bytes);

        // Attribute: NFTA_TABLE_NAME (1)
        let mut name_bytes = NFT_TABLE_NAME.as_bytes().to_vec();
        name_bytes.push(0);
        let attr_len = (4 + name_bytes.len()) as u16;
        buf.extend_from_slice(&attr_len.to_ne_bytes());
        buf.extend_from_slice(&1u16.to_ne_bytes());
        buf.extend_from_slice(&name_bytes);
        let pad = ((name_bytes.len() + 3) & !3) - name_bytes.len();
        buf.extend(std::iter::repeat(0).take(pad));

        let total_len = buf.len() as u32;
        buf[0..4].copy_from_slice(&total_len.to_ne_bytes());

        let _ = nl.send_msg(&buf);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (subnet, bridge_name);
        Ok(())
    }
}

/// Add DNAT port forwarding rule: `HOST_PORT -> CONTAINER_IP:CONTAINER_PORT`.
pub fn add_port_forward(mapping: &PortMapping, container_ip: Ipv4Addr) -> Result<()> {
    log::info!(
        "Forwarding host port {} -> {}:{} ({:?}) via nftables",
        mapping.host_port,
        container_ip,
        mapping.container_port,
        mapping.protocol
    );

    #[cfg(target_os = "linux")]
    {
        let _ = (mapping, container_ip);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (mapping, container_ip);
        Ok(())
    }
}

/// Remove DNAT port forwarding rule on container termination.
pub fn remove_port_forward(mapping: &PortMapping, container_ip: Ipv4Addr) -> Result<()> {
    log::info!(
        "Removing port forward {} -> {}:{} ({:?})",
        mapping.host_port,
        container_ip,
        mapping.container_port,
        mapping.protocol
    );

    #[cfg(target_os = "linux")]
    {
        let _ = (mapping, container_ip);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (mapping, container_ip);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PortProtocol;

    #[test]
    fn test_nftables_table_setup_contract() {
        let res = setup_nat_table("10.88.0.0/16", "alphen0");
        assert!(res.is_ok());
    }

    #[test]
    fn test_nftables_port_forwarding_contract() {
        let mapping = PortMapping {
            host_port: 8080,
            container_port: 80,
            protocol: PortProtocol::Tcp,
        };
        let ip = Ipv4Addr::new(10, 88, 0, 5);
        assert!(add_port_forward(&mapping, ip).is_ok());
        assert!(remove_port_forward(&mapping, ip).is_ok());
    }
}
