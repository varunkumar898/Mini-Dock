//! Direct kernel netlink socket driver for container interfaces (`alphen0`, veth, routing).
//!
//! Talks directly to the Linux kernel via `AF_NETLINK` / `NETLINK_ROUTE` raw sockets.
//! No external processes (`ip`, `ifconfig`, `brctl`) are ever executed.
//!
//! References:
//! - `netlink(7)`: Linux Netlink communication mechanism
//! - `rtnetlink(7)`: Linux IPv4/IPv6 routing socket
//! - Linux kernel `include/uapi/linux/rtnetlink.h`, `if_link.h`

#[cfg(target_os = "linux")]
use alphen_common::AlphenError;
use alphen_common::Result;
use std::net::Ipv4Addr;

#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::mem;
#[cfg(target_os = "linux")]
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub const BRIDGE_NAME: &str = "alphen0";

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    // Netlink constants from linux/rtnetlink.h & if_link.h
    pub const RTM_NEWLINK: u16 = 16;
    pub const RTM_DELLINK: u16 = 17;
    pub const RTM_GETLINK: u16 = 18;
    pub const RTM_NEWADDR: u16 = 20;
    pub const RTM_NEWROUTE: u16 = 24;

    pub const NLM_F_REQUEST: u16 = 1;
    pub const NLM_F_ACK: u16 = 4;
    pub const NLM_F_CREATE: u16 = 0x400;
    pub const NLM_F_EXCL: u16 = 0x200;

    pub const NLMSG_ERROR: u16 = 2;

    pub const IFLA_IFNAME: u16 = 3;
    pub const IFLA_MASTER: u16 = 10;
    pub const IFLA_LINKINFO: u16 = 18;
    pub const IFLA_NET_NS_PID: u16 = 19;

    pub const IFLA_INFO_KIND: u16 = 1;
    pub const IFLA_INFO_DATA: u16 = 2;

    pub const VETH_INFO_PEER: u16 = 1;

    pub const IFA_ADDRESS: u16 = 1;
    pub const IFA_LOCAL: u16 = 2;

    pub const RTA_DST: u16 = 1;
    pub const RTA_GATEWAY: u16 = 5;
    pub const RTA_OIF: u16 = 4;

    pub const RT_TABLE_MAIN: u8 = 254;
    pub const RTPROT_BOOT: u8 = 3;
    pub const RT_SCOPE_UNIVERSE: u8 = 0;
    pub const RTN_UNICAST: u8 = 1;

    pub const IFF_UP: u32 = 1;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct NlMsgHdr {
        pub nlmsg_len: u32,
        pub nlmsg_type: u16,
        pub nlmsg_flags: u16,
        pub nlmsg_seq: u32,
        pub nlmsg_pid: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct NlMsgErr {
        pub error: i32,
        pub msg: NlMsgHdr,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct IfInfoMsg {
        pub ifi_family: u8,
        pub __pad: u8,
        pub ifi_type: u16,
        pub ifi_index: i32,
        pub ifi_flags: u32,
        pub ifi_change: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct IfAddrMsg {
        pub ifa_family: u8,
        pub ifa_prefixlen: u8,
        pub ifa_flags: u8,
        pub ifa_scope: u8,
        pub ifa_index: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct RtMsg {
        pub rtm_family: u8,
        pub rtm_dst_len: u8,
        pub rtm_src_len: u8,
        pub rtm_tos: u8,
        pub rtm_table: u8,
        pub rtm_protocol: u8,
        pub rtm_scope: u8,
        pub rtm_type: u8,
        pub rtm_flags: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub struct RtAttr {
        pub rta_len: u16,
        pub rta_type: u16,
    }

    pub struct NetlinkSocket {
        fd: RawFd,
        seq: u32,
    }

    impl NetlinkSocket {
        pub fn open() -> Result<Self> {
            // SAFETY: Open raw AF_NETLINK socket for NETLINK_ROUTE
            let fd = unsafe {
                libc::socket(
                    libc::AF_NETLINK,
                    libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                    libc::NETLINK_ROUTE,
                )
            };
            if fd < 0 {
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }

            let mut sa: libc::sockaddr_nl = unsafe { mem::zeroed() };
            sa.nl_family = libc::AF_NETLINK as u16;
            sa.nl_pid = 0; // Kernel will assign
            sa.nl_groups = 0;

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

            Ok(Self { fd, seq: 1 })
        }

        pub fn send_and_wait_ack(&mut self, buf: &[u8]) -> Result<()> {
            let ret =
                unsafe { libc::send(self.fd, buf.as_ptr() as *const libc::c_void, buf.len(), 0) };
            if ret < 0 {
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }

            let mut resp = [0u8; 4096];
            let n = unsafe {
                libc::recv(
                    self.fd,
                    resp.as_mut_ptr() as *mut libc::c_void,
                    resp.len(),
                    0,
                )
            };
            if n < 0 {
                return Err(AlphenError::System(std::io::Error::last_os_error()));
            }

            if n >= mem::size_of::<NlMsgHdr>() as isize {
                let hdr = unsafe { *(resp.as_ptr() as *const NlMsgHdr) };
                if hdr.nlmsg_type == NLMSG_ERROR {
                    let err = unsafe {
                        *(resp.as_ptr().add(mem::size_of::<NlMsgHdr>()) as *const NlMsgErr)
                    };
                    if err.error != 0 {
                        // Negative errno
                        let os_err = -err.error;
                        // libc::EEXIST (17) is normal when device already exists
                        if os_err != libc::EEXIST {
                            return Err(AlphenError::Runtime(format!(
                                "Netlink command error: {} (errno {os_err})",
                                std::io::Error::from_raw_os_error(os_err)
                            )));
                        }
                    }
                }
            }

            Ok(())
        }
    }

    impl Drop for NetlinkSocket {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.fd);
            }
        }
    }

    pub fn align4(len: usize) -> usize {
        (len + 3) & !3
    }

    pub fn append_rta(buf: &mut Vec<u8>, rta_type: u16, data: &[u8]) {
        let rta_len = (mem::size_of::<RtAttr>() + data.len()) as u16;
        let attr = RtAttr { rta_len, rta_type };
        let attr_bytes = unsafe {
            std::slice::from_raw_parts(&attr as *const _ as *const u8, mem::size_of::<RtAttr>())
        };
        buf.extend_from_slice(attr_bytes);
        buf.extend_from_slice(data);
        let pad = align4(data.len()) - data.len();
        buf.extend(std::iter::repeat(0).take(pad));
    }
}

/// Ensure bridge `alphen0` exists and has gateway IP assigned.
pub fn ensure_bridge(bridge_name: &str, gateway_ip: Ipv4Addr) -> Result<u32> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let mut nl = NetlinkSocket::open()?;

        // 1. Create bridge interface: RTM_NEWLINK with IFLA_LINKINFO kind="bridge"
        let mut buf = Vec::with_capacity(512);
        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        };

        // Header placeholder
        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        };
        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        // IFLA_IFNAME
        let mut name_bytes = bridge_name.as_bytes().to_vec();
        name_bytes.push(0);
        append_rta(&mut buf, IFLA_IFNAME, &name_bytes);

        // Nested IFLA_LINKINFO -> IFLA_INFO_KIND = "bridge\0"
        let mut linkinfo_buf = Vec::new();
        let mut kind_bytes = b"bridge\0".to_vec();
        append_rta(&mut linkinfo_buf, IFLA_INFO_KIND, &kind_bytes);
        append_rta(&mut buf, IFLA_LINKINFO, &linkinfo_buf);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        // Send create link (ignore EEXIST)
        let _ = nl.send_and_wait_ack(&buf);

        // Get bridge interface index
        let bridge_ifindex = unsafe {
            let name_c = CString::new(bridge_name).unwrap();
            libc::if_nametoindex(name_c.as_ptr())
        };
        if bridge_ifindex == 0 {
            return Err(AlphenError::Runtime(format!(
                "Failed to get index for bridge {bridge_name}"
            )));
        }

        // 2. Set bridge interface UP
        set_link_up_by_index(bridge_ifindex)?;

        // 3. Assign gateway IP 10.88.0.1/16 to bridge
        add_address(bridge_ifindex, gateway_ip, 16)?;

        Ok(bridge_ifindex)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (bridge_name, gateway_ip);
        Ok(42) // Simulated bridge ifindex
    }
}

/// Set a network link UP by interface index.
pub fn set_link_up_by_index(ifindex: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: ifindex as i32,
            ifi_flags: IFF_UP,
            ifi_change: IFF_UP,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: (mem::size_of::<NlMsgHdr>() + mem::size_of::<IfInfoMsg>()) as u32,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 2,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        nl.send_and_wait_ack(&buf)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = ifindex;
        Ok(())
    }
}

/// Set a network link UP by interface name.
pub fn set_link_up(link_name: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let c_name = CString::new(link_name).unwrap();
        let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if idx == 0 {
            return Err(AlphenError::Runtime(format!(
                "Network interface {link_name} not found"
            )));
        }
        set_link_up_by_index(idx)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = link_name;
        Ok(())
    }
}

/// Add an IPv4 address and prefix to an interface.
pub fn add_address(ifindex: u32, ip: Ipv4Addr, prefixlen: u8) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let ifa = IfAddrMsg {
            ifa_family: libc::AF_INET as u8,
            ifa_prefixlen: prefixlen,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: ifindex,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWADDR,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            nlmsg_seq: 3,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifa_bytes = unsafe {
            std::slice::from_raw_parts(&ifa as *const _ as *const u8, mem::size_of::<IfAddrMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifa_bytes);

        let ip_bytes = ip.octets();
        append_rta(&mut buf, IFA_LOCAL, &ip_bytes);
        append_rta(&mut buf, IFA_ADDRESS, &ip_bytes);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        let _ = nl.send_and_wait_ack(&buf);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ifindex, ip, prefixlen);
        Ok(())
    }
}

/// Add default route `0.0.0.0/0 via gateway dev oif`.
pub fn add_default_route(gateway: Ipv4Addr, oif: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let rtm = RtMsg {
            rtm_family: libc::AF_INET as u8,
            rtm_dst_len: 0, // default route: 0.0.0.0/0
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: RT_TABLE_MAIN,
            rtm_protocol: RTPROT_BOOT,
            rtm_scope: RT_SCOPE_UNIVERSE,
            rtm_type: RTN_UNICAST,
            rtm_flags: 0,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWROUTE,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE,
            nlmsg_seq: 4,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let rtm_bytes = unsafe {
            std::slice::from_raw_parts(&rtm as *const _ as *const u8, mem::size_of::<RtMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(rtm_bytes);

        let gw_bytes = gateway.octets();
        append_rta(&mut buf, RTA_GATEWAY, &gw_bytes);
        let oif_bytes = oif.to_ne_bytes();
        append_rta(&mut buf, RTA_OIF, &oif_bytes);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        let _ = nl.send_and_wait_ack(&buf);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (gateway, oif);
        Ok(())
    }
}

/// Create a veth pair: `host_veth` and `peer_veth`.
pub fn create_veth_pair(host_veth: &str, peer_veth: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(512);

        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK | NLM_F_CREATE | NLM_F_EXCL,
            nlmsg_seq: 5,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        let mut host_name_bytes = host_veth.as_bytes().to_vec();
        host_name_bytes.push(0);
        append_rta(&mut buf, IFLA_IFNAME, &host_name_bytes);

        // Nested IFLA_LINKINFO
        let mut linkinfo_buf = Vec::new();
        let mut kind_bytes = b"veth\0".to_vec();
        append_rta(&mut linkinfo_buf, IFLA_INFO_KIND, &kind_bytes);

        // Nested VETH_INFO_PEER inside IFLA_INFO_DATA
        let mut peer_ifi_buf = Vec::new();
        let peer_ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: 0,
            ifi_flags: 0,
            ifi_change: 0,
        };
        let peer_bytes = unsafe {
            std::slice::from_raw_parts(
                &peer_ifi as *const _ as *const u8,
                mem::size_of::<IfInfoMsg>(),
            )
        };
        peer_ifi_buf.extend_from_slice(peer_bytes);
        let mut peer_name_bytes = peer_veth.as_bytes().to_vec();
        peer_name_bytes.push(0);
        append_rta(&mut peer_ifi_buf, IFLA_IFNAME, &peer_name_bytes);

        let mut info_data_buf = Vec::new();
        append_rta(&mut info_data_buf, VETH_INFO_PEER, &peer_ifi_buf);
        append_rta(&mut linkinfo_buf, IFLA_INFO_DATA, &info_data_buf);

        append_rta(&mut buf, IFLA_LINKINFO, &linkinfo_buf);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        nl.send_and_wait_ack(&buf)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (host_veth, peer_veth);
        Ok(())
    }
}

/// Attach an interface to a master bridge.
pub fn set_link_master(link_name: &str, master_ifindex: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let c_name = CString::new(link_name).unwrap();
        let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if idx == 0 {
            return Err(AlphenError::Runtime(format!("Link {link_name} not found")));
        }

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: idx as i32,
            ifi_flags: 0,
            ifi_change: 0,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 6,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        let master_bytes = master_ifindex.to_ne_bytes();
        append_rta(&mut buf, IFLA_MASTER, &master_bytes);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        nl.send_and_wait_ack(&buf)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (link_name, master_ifindex);
        Ok(())
    }
}

/// Move a network interface into a target container process's network namespace.
pub fn set_link_netns_pid(link_name: &str, pid: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let c_name = CString::new(link_name).unwrap();
        let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if idx == 0 {
            return Err(AlphenError::Runtime(format!("Link {link_name} not found")));
        }

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: idx as i32,
            ifi_flags: 0,
            ifi_change: 0,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_NEWLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 7,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        let pid_bytes = pid.to_ne_bytes();
        append_rta(&mut buf, IFLA_NET_NS_PID, &pid_bytes);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        nl.send_and_wait_ack(&buf)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (link_name, pid);
        Ok(())
    }
}

/// Delete a network interface by name (e.g. host veth endpoint).
pub fn delete_link(link_name: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use linux::*;

        let c_name = CString::new(link_name).unwrap();
        let idx = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if idx == 0 {
            return Ok(()); // Already deleted
        }

        let mut nl = NetlinkSocket::open()?;
        let mut buf = Vec::with_capacity(128);

        let ifi = IfInfoMsg {
            ifi_family: libc::AF_UNSPEC as u8,
            __pad: 0,
            ifi_type: 0,
            ifi_index: idx as i32,
            ifi_flags: 0,
            ifi_change: 0,
        };

        let hdr = NlMsgHdr {
            nlmsg_len: 0,
            nlmsg_type: RTM_DELLINK,
            nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
            nlmsg_seq: 8,
            nlmsg_pid: 0,
        };

        let hdr_bytes = unsafe {
            std::slice::from_raw_parts(&hdr as *const _ as *const u8, mem::size_of::<NlMsgHdr>())
        };
        let ifi_bytes = unsafe {
            std::slice::from_raw_parts(&ifi as *const _ as *const u8, mem::size_of::<IfInfoMsg>())
        };
        buf.extend_from_slice(hdr_bytes);
        buf.extend_from_slice(ifi_bytes);

        let total_len = buf.len() as u32;
        let len_slice = total_len.to_ne_bytes();
        buf[0..4].copy_from_slice(&len_slice);

        let _ = nl.send_and_wait_ack(&buf);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = link_name;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_bridge_contract() {
        let ip = Ipv4Addr::new(10, 88, 0, 1);
        let res = ensure_bridge(BRIDGE_NAME, ip);
        assert!(res.is_ok());
    }

    #[test]
    fn test_create_veth_pair_contract() {
        let res = create_veth_pair("test_veth0", "test_peer0");
        assert!(res.is_ok());
    }
}
