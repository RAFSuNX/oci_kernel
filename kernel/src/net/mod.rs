// Hardware-dependent modules: only compiled for the bare-metal kernel.
#[cfg(not(test))] pub mod virtio_net;
#[cfg(not(test))] pub mod stack;
#[cfg(not(test))] pub mod tls;
#[cfg(not(test))] pub mod http;

// Pure-logic modules: always compiled (have unit tests).
pub mod vswitch;

#[cfg(not(test))]
use spin::{Mutex, Once};
#[cfg(not(test))]
use stack::NetworkStack;
#[cfg(not(test))]
use smoltcp::socket::tcp::{Socket as TcpSocket, State as TcpState};
#[cfg(not(test))]
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

/// The global network stack — initialised once in `net::init`.
#[cfg(not(test))]
static NETWORK: Once<Mutex<NetworkStack>> = Once::new();

/// Handle of the active HTTP TCP socket (set by `setup_http_listener`).
#[cfg(not(test))]
static HTTP_HANDLE: Once<smoltcp::iface::SocketHandle> = Once::new();

/// Monotonic tick counter — incremented on every smoltcp poll call.
#[cfg(not(test))]
static NET_TICKS: AtomicU64 = AtomicU64::new(0);

/// Port the HTTP listener is bound to (set by `setup_http_listener`).
#[cfg(not(test))]
static HTTP_PORT: AtomicU16 = AtomicU16::new(0);

/// True once we've sent a response for the current connection;
/// reset to false when the socket re-enters the Closed/TimeWait state.
#[cfg(not(test))]
static HTTP_RESPONDED: AtomicBool = AtomicBool::new(false);

/// Minimal HTTP/1.0 placeholder response served by the kernel TCP stack.
/// M2 will replace this with real container stdout proxying.
#[cfg(not(test))]
const HTTP_RESPONSE: &[u8] = b"HTTP/1.0 200 OK\r\n\
Content-Type: text/html\r\n\
Connection: close\r\n\
\r\n\
<!DOCTYPE html><html>\
<head><title>Welcome to nginx! -- OCI Kernel</title></head>\
<body>\
<h1>Welcome to nginx!</h1>\
<p><strong>OCI Kernel 0.1.0</strong> -- the kernel IS the container runtime.</p>\
<p>Container: <code>nginx:latest</code> (id 1) -- Port 80</p>\
<hr/>\
<p><em>Milestone 2 will proxy traffic to the real nginx process.<br/>\
This response is served directly by the kernel TCP stack.</em></p>\
</body></html>";

/// Called once from `kernel_main` after `memory::init`.
#[cfg(not(test))]
pub fn init(phys_offset: u64) {
    let device = virtio_net::VirtioNet::probe(phys_offset)
        .expect("virtio-net device not found — start QEMU with -device virtio-net-pci");
    let mac = device.mac;
    let stack = NetworkStack::init(device);
    NETWORK.call_once(|| Mutex::new(stack));
    crate::serial_println!(
        "[OK] Network (10.0.2.15/24) MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
}

/// Set up a TCP listener on `port` that serves an HTTP placeholder page.
/// Must be called after `init()`. Idempotent — only the first call takes effect.
#[cfg(not(test))]
pub fn setup_http_listener(port: u16) {
    HTTP_PORT.store(port, Ordering::Relaxed);
    if let Some(net) = NETWORK.get() {
        let handle = net.lock().setup_tcp_listener(port);
        HTTP_HANDLE.call_once(|| handle);
        crate::serial_println!("[OK] HTTP listener on port {}", port);
    }
}

/// Poll the network and, if an HTTP connection is active, handle one exchange.
///
/// This is called in the serial read hot-loop so the kernel serves HTTP
/// requests while the operator is idle at the shell prompt.
/// Uses `try_lock` so it never blocks the shell.
#[cfg(not(test))]
pub fn serve_http_once() {
    let Some(net) = NETWORK.get() else { return };
    let Some(&handle) = HTTP_HANDLE.get() else { return };
    let Some(mut stack) = net.try_lock() else { return };

    let t = NET_TICKS.fetch_add(1, Ordering::Relaxed);
    let ts = smoltcp::time::Instant::from_millis(t as i64);
    stack.poll(ts); // calls iface.poll internally — avoids multi-borrow through MutexGuard

    let port      = HTTP_PORT.load(Ordering::Relaxed);
    let responded = HTTP_RESPONDED.load(Ordering::Relaxed);
    let socket    = stack.sockets.get_mut::<TcpSocket>(handle);

    match socket.state() {
        // Socket closed (or timed-out after FIN exchange) — re-arm the listener.
        TcpState::Closed | TcpState::TimeWait => {
            socket.listen(port).ok();
            HTTP_RESPONDED.store(false, Ordering::Relaxed);
        }

        // Connection established: drain the HTTP request and send our response.
        TcpState::Established if !responded => {
            if socket.can_recv() {
                // Consume all incoming bytes (we don't need to parse the request).
                loop {
                    match socket.recv(|buf| (buf.len(), buf.len())) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if !socket.can_recv() { break; }
                }
                // Send the HTTP response and half-close our side.
                if socket.can_send() {
                    socket.send_slice(HTTP_RESPONSE).ok();
                    socket.close(); // sends FIN after the response is flushed
                    HTTP_RESPONDED.store(true, Ordering::Relaxed);
                }
            }
        }

        _ => {} // SynReceived, FinWait, CloseWait, etc. — let smoltcp handle it
    }
}

/// Poll the network stack (called from the timer interrupt handler or manually).
#[cfg(not(test))]
pub fn poll() {
    if let Some(net) = NETWORK.get() {
        if let Some(mut stack) = net.try_lock() {
            let t = NET_TICKS.fetch_add(1, Ordering::Relaxed);
            stack.poll(smoltcp::time::Instant::from_millis(t as i64));
        }
    }
}
