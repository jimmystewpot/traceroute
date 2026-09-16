//! Safe abstraction over Linux MSG_ERRQUEUE for reading ICMP errors from UDP sockets.
//! Required for unprivileged traceroute on Linux.
//! Documented and written in Australian English.

use socket2::Socket;
use std::net::IpAddr;

/// Enables IP_RECVERR (IPv4) or IPV6_RECVERR (IPv6) on the given socket.
///
/// When enabled, the kernel delivers ICMP error messages (such as Time Exceeded
/// and Port Unreachable) through the socket error queue rather than discarding them.
/// This is the standard approach for unprivileged traceroute on Linux.
///
/// Silently succeeds on non-Linux platforms (no-op stub).
///
/// # Errors
/// Returns an `std::io::Error` if the `setsockopt` system call fails.
#[cfg(target_os = "linux")]
pub fn enable_ip_recverr(socket: &Socket) -> std::io::Result<()> {
    use libc::{IPPROTO_IP, IPPROTO_IPV6, IPV6_RECVERR, IP_RECVERR};
    use std::os::unix::io::AsRawFd;

    let fd = socket.as_raw_fd();
    // Determine whether the socket is IPv4 or IPv6 by inspecting the local address.
    // We rely on the socket domain (AF_INET vs AF_INET6) via a zero-length getsockname probe.
    // A more robust approach is to pass the domain to this function, but we infer it here
    // by attempting to read the local address family from the socket.
    let domain = get_socket_domain(socket)?;

    // Safety: fd is a valid file descriptor obtained from a live Socket.
    // The option value (1i32) has the correct type and alignment for setsockopt.
    let optval: libc::c_int = 1;
    let ret = unsafe {
        if domain == libc::AF_INET {
            libc::setsockopt(
                fd,
                IPPROTO_IP,
                IP_RECVERR,
                &optval as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        } else {
            libc::setsockopt(
                fd,
                IPPROTO_IPV6,
                IPV6_RECVERR,
                &optval as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        }
    };

    if ret == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Determines the address family (AF_INET or AF_INET6) of a bound socket.
///
/// Uses `getsockname` to retrieve the local address and reads the `sa_family` field.
#[cfg(target_os = "linux")]
fn get_socket_domain(socket: &Socket) -> std::io::Result<libc::c_int> {
    use std::os::unix::io::AsRawFd;

    let fd = socket.as_raw_fd();
    // Safety: buf is large enough to hold any sockaddr variant; we only read sa_family.
    let mut addr_storage: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut addr_len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockname(
            fd,
            &mut addr_storage as *mut libc::sockaddr_storage as *mut libc::sockaddr,
            &mut addr_len,
        )
    };
    if ret == -1 {
        // If getsockname fails (e.g. socket is not yet bound), default to AF_INET.
        // This is a safe conservative fallback.
        return Ok(libc::AF_INET);
    }
    Ok(addr_storage.ss_family as libc::c_int)
}

/// Reads one error from the socket error queue using `MSG_ERRQUEUE`.
///
/// Returns `Some((responding_router_addr, icmp_type, icmp_code))` if an ICMP error
/// is present in the error queue, or `None` if the queue is empty or
/// on non-Linux platforms.
///
/// # Safety
/// Calls `libc::recvmsg` with a carefully constructed message buffer. The
/// buffer layout matches the kernel's `sock_extended_err` ancillary message
/// format as documented in `ip(7)` and `linux/errqueue.h`.
#[cfg(target_os = "linux")]
pub fn recv_from_errqueue(socket: &Socket) -> Option<(IpAddr, u8, u8)> {
    use libc::{
        c_void, cmsghdr, iovec, msghdr, recvmsg, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6,
        IPPROTO_IP, IPPROTO_IPV6, IPV6_RECVERR, IP_RECVERR, MSG_ERRQUEUE, SO_EE_ORIGIN_ICMP,
        SO_EE_ORIGIN_ICMP6,
    };
    use std::mem;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::os::unix::io::AsRawFd;

    let fd = socket.as_raw_fd();

    // Payload buffer: we do not care about the actual UDP payload echo;
    // only the ancillary (control) data contains the ICMP error information.
    let mut payload = [0u8; 64];
    let mut iov = iovec {
        iov_base: payload.as_mut_ptr() as *mut c_void,
        iov_len: payload.len(),
    };

    // Control buffer must be large enough for cmsghdr + sock_extended_err + sockaddr_in6.
    // 256 bytes comfortably accommodates any platform variant.
    let mut control_buf = [0u8; 256];
    // Safety: zeroing a POD struct is always valid.
    let mut msg: msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control_buf.as_mut_ptr() as *mut c_void;
    msg.msg_controllen = control_buf.len();

    // MSG_ERRQUEUE returns immediately (non-blocking) regardless of the socket
    // blocking mode. Returns -1 with errno=EAGAIN if the queue is empty.
    // Safety: msg is correctly initialised; fd is a valid socket descriptor.
    let ret = unsafe { recvmsg(fd, &mut msg, MSG_ERRQUEUE) };
    if ret < 0 {
        return None;
    }

    // Walk ancillary (control) message headers to find the sock_extended_err record.
    // Safety: CMSG_FIRSTHDR reads msg.msg_control and msg.msg_controllen which were
    // set above; all pointer arithmetic follows the POSIX cmsg(3) contract.
    let mut cmsg_ptr = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg_ptr.is_null() {
        let cmsg: &cmsghdr = unsafe { &*cmsg_ptr };

        let is_ipv4_recverr = cmsg.cmsg_level == IPPROTO_IP && cmsg.cmsg_type == IP_RECVERR;
        let is_ipv6_recverr = cmsg.cmsg_level == IPPROTO_IPV6 && cmsg.cmsg_type == IPV6_RECVERR;

        if is_ipv4_recverr || is_ipv6_recverr {
            // Safety: CMSG_DATA points to the payload of a valid cmsghdr that we
            // just validated above. The kernel guarantees that a sock_extended_err
            // immediately follows the cmsghdr for IP_RECVERR / IPV6_RECVERR messages.
            let ee_ptr = unsafe { libc::CMSG_DATA(cmsg_ptr) as *const libc::sock_extended_err };

            // Validate that enough data is present for sock_extended_err and the trailing sockaddr.
            // IPv6 error records carry a sockaddr_in6 (28 bytes) whereas IPv4 records carry a
            // sockaddr_in (16 bytes). Using the wrong size would silently accept under-sized
            // IPv6 control messages, leading to out-of-bounds reads via SO_EE_OFFENDER.
            let ee_size = mem::size_of::<libc::sock_extended_err>();
            let sa_size = if is_ipv6_recverr {
                mem::size_of::<libc::sockaddr_in6>()
            } else {
                mem::size_of::<libc::sockaddr_in>()
            };
            let cmsg_data_len = cmsg.cmsg_len as usize;
            if cmsg_data_len < mem::size_of::<cmsghdr>() + ee_size + sa_size {
                // Insufficient data — skip this cmsg.
                cmsg_ptr = unsafe { libc::CMSG_NXTHDR(&msg, cmsg_ptr) };
                continue;
            }

            let ee: &libc::sock_extended_err = unsafe { &*ee_ptr };
            let icmp_type = ee.ee_type;
            let icmp_code = ee.ee_code;
            let origin = ee.ee_origin;

            // Only process errors originating from ICMP or ICMPv6.
            if origin != SO_EE_ORIGIN_ICMP && origin != SO_EE_ORIGIN_ICMP6 {
                cmsg_ptr = unsafe { libc::CMSG_NXTHDR(&msg, cmsg_ptr) };
                continue;
            }

            // SO_EE_OFFENDER yields the sockaddr of the router that generated the error.
            // Safety: SO_EE_OFFENDER computes ee_ptr.offset(1) which the kernel guarantees
            // is a valid sockaddr immediately following sock_extended_err in the cmsg payload.
            let offender_ptr = unsafe { libc::SO_EE_OFFENDER(ee_ptr) };
            if offender_ptr.is_null() {
                cmsg_ptr = unsafe { libc::CMSG_NXTHDR(&msg, cmsg_ptr) };
                continue;
            }

            let sa_family = unsafe { (*offender_ptr).sa_family } as libc::c_int;

            let router_ip = if sa_family == AF_INET {
                // Safety: sa_family is AF_INET so the sockaddr is a sockaddr_in.
                let sin = unsafe { &*(offender_ptr as *const sockaddr_in) };
                IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)))
            } else if sa_family == AF_INET6 {
                // Safety: sa_family is AF_INET6 so the sockaddr is a sockaddr_in6.
                let sin6 = unsafe { &*(offender_ptr as *const sockaddr_in6) };
                IpAddr::V6(Ipv6Addr::from(sin6.sin6_addr.s6_addr))
            } else {
                // Unknown address family — cannot represent as IpAddr.
                cmsg_ptr = unsafe { libc::CMSG_NXTHDR(&msg, cmsg_ptr) };
                continue;
            };

            return Some((router_ip, icmp_type, icmp_code));
        }

        cmsg_ptr = unsafe { libc::CMSG_NXTHDR(&msg, cmsg_ptr) };
    }

    None
}

/// Non-Linux stub: always returns `None` since MSG_ERRQUEUE is Linux-specific.
#[cfg(not(target_os = "linux"))]
pub fn recv_from_errqueue(_socket: &Socket) -> Option<(IpAddr, u8, u8)> {
    None
}

/// Non-Linux stub: always succeeds since IP_RECVERR is Linux-specific.
///
/// On non-Linux platforms, ICMP errors are typically delivered through
/// standard socket receive paths or are not available to unprivileged processes.
#[cfg(not(target_os = "linux"))]
pub fn enable_ip_recverr(_socket: &Socket) -> std::io::Result<()> {
    Ok(())
}
