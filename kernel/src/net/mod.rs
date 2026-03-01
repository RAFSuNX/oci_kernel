pub mod virtio_net;
pub mod stack;
pub mod tls;
pub mod http;

use spin::{Mutex, Once};
use stack::NetworkStack;

static NETWORK: Once<Mutex<NetworkStack>> = Once::new();

/// Called once from kernel_main after memory::init.
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

/// Poll the network stack. Called from the timer interrupt handler.
pub fn poll() {
    if let Some(net) = NETWORK.get() {
        if let Some(mut stack) = net.try_lock() {
            static TICKS: core::sync::atomic::AtomicU64 =
                core::sync::atomic::AtomicU64::new(0);
            let t = TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            stack.poll(smoltcp::time::Instant::from_millis(t as i64));
        }
    }
}
