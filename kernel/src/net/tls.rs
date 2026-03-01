extern crate alloc;

use alloc::vec::Vec;

use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer};
use smoltcp::wire::IpEndpoint;

use crate::net::NETWORK;

/// Minimal kernel RNG using RDRAND instruction.
pub struct KernelRng;

impl rand_core::RngCore for KernelRng {
    fn next_u32(&mut self) -> u32 {
        let mut val = 0u64;
        loop {
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut val) } == 1 {
                return val as u32;
            }
        }
    }
    fn next_u64(&mut self) -> u64 {
        let mut val = 0u64;
        loop {
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut val) } == 1 {
                return val;
            }
        }
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut i = 0;
        while i < dest.len() {
            let v = self.next_u64().to_le_bytes();
            let n = (dest.len() - i).min(8);
            dest[i..i + n].copy_from_slice(&v[..n]);
            i += n;
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// KernelRng uses RDRAND which is cryptographically secure.
impl rand_core::CryptoRng for KernelRng {}

#[derive(Debug)]
pub enum TlsConnectError {
    NoNetwork,
    TcpFailed,
    TlsError,
}

/// Connect to `host:port`, send `request` bytes, and return all response bytes.
///
/// This is a single-shot request: connect, send, read until close, disconnect.
///
/// # Current implementation
/// Plain TCP only (no TLS). TLS 1.3 wrapping via `embedded-tls` is a TODO
/// pending a smoltcp socket adapter that implements `embedded_io::{Read, Write}`.
pub fn https_request(host: &str, port: u16, request: &[u8]) -> Result<Vec<u8>, TlsConnectError> {
    let net = NETWORK.get().ok_or(TlsConnectError::NoNetwork)?;
    let mut stack = net.lock();

    // Resolve hostname to an IP address.
    // DNS resolution is added in Task 10; hardcoded entries are used for now.
    let ip = resolve_host(host).ok_or(TlsConnectError::TcpFailed)?;
    let endpoint = IpEndpoint::new(ip, port);

    // Add a TCP socket to the smoltcp socket set.
    let tcp_handle = {
        let socket = TcpSocket::new(
            SocketBuffer::new(alloc::vec![0u8; 4096]),
            SocketBuffer::new(alloc::vec![0u8; 16384]),
        );
        stack.sockets.add(socket)
    };

    // Connect.
    // Destructure to split borrows: iface and sockets are independent fields.
    {
        let crate::net::stack::NetworkStack { ref mut iface, ref mut sockets, .. } = *stack;
        let cx = iface.context();
        let socket = sockets.get_mut::<TcpSocket>(tcp_handle);
        socket
            .connect(cx, endpoint, 49152u16)
            .map_err(|_| TlsConnectError::TcpFailed)?;
    }

    // Poll until the connection is established.
    for _ in 0..100_000 {
        stack.poll(smoltcp::time::Instant::from_millis(0));
        let socket = stack.sockets.get::<TcpSocket>(tcp_handle);
        if socket.is_active() {
            break;
        }
    }

    // Send the request.
    {
        let socket = stack.sockets.get_mut::<TcpSocket>(tcp_handle);
        socket
            .send_slice(request)
            .map_err(|_| TlsConnectError::TcpFailed)?;
    }

    // Poll and accumulate the response until the server closes the connection.
    let mut response = Vec::new();
    loop {
        stack.poll(smoltcp::time::Instant::from_millis(0));
        let socket = stack.sockets.get_mut::<TcpSocket>(tcp_handle);
        let mut chunk = [0u8; 1024];
        match socket.recv_slice(&mut chunk) {
            Ok(0) => {}
            Ok(n) => response.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
        if !socket.may_recv() {
            break;
        }
    }

    // Remove the socket from the set.
    stack.sockets.remove(tcp_handle);
    Ok(response)
}

/// Resolve a hostname to a smoltcp `IpAddress`.
///
/// DNS resolution is not yet implemented (Task 10). Known Docker Hub endpoints
/// are hardcoded here as a temporary measure.
fn resolve_host(host: &str) -> Option<smoltcp::wire::IpAddress> {
    use smoltcp::wire::{IpAddress, Ipv4Address};
    match host {
        "registry-1.docker.io" => Some(IpAddress::Ipv4(Ipv4Address::new(54, 236, 246, 36))),
        "auth.docker.io" => Some(IpAddress::Ipv4(Ipv4Address::new(54, 236, 246, 36))),
        _ => None,
    }
}
