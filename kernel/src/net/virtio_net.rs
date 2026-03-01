extern crate alloc;

use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

// PCI config space access via I/O ports
const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr: u32 = 0x8000_0000
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) <<  8)
        | ((offset & 0xFC) as u32);
    unsafe {
        Port::<u32>::new(PCI_CONFIG_ADDR).write(addr);
        PortReadOnly::<u32>::new(PCI_CONFIG_DATA).read()
    }
}

fn pci_write32(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr: u32 = 0x8000_0000
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) <<  8)
        | ((offset & 0xFC) as u32);
    unsafe {
        Port::<u32>::new(PCI_CONFIG_ADDR).write(addr);
        Port::<u32>::new(PCI_CONFIG_DATA).write(value);
    }
}

// virtio-net legacy I/O register offsets from BAR0
const VIRTIO_PCI_HOST_FEATURES:  u16 = 0;
const VIRTIO_PCI_GUEST_FEATURES: u16 = 4;
const VIRTIO_PCI_QUEUE_ADDR:     u16 = 8;  // queue PFN (in 4096-byte pages)
const VIRTIO_PCI_QUEUE_SIZE:     u16 = 12;
const VIRTIO_PCI_QUEUE_SELECT:   u16 = 14;
const VIRTIO_PCI_QUEUE_NOTIFY:   u16 = 16;
const VIRTIO_PCI_STATUS:         u16 = 18;
const VIRTIO_PCI_ISR:            u16 = 19;
// virtio-net device-specific config at +20
const VIRTIO_NET_CONFIG_MAC: u16 = 20;

const VIRTIO_STATUS_ACKNOWLEDGE: u8 = 1;
const VIRTIO_STATUS_DRIVER:      u8 = 2;
const VIRTIO_STATUS_DRIVER_OK:   u8 = 4;
const VIRTIO_STATUS_FEATURES_OK: u8 = 8;

// virtio-net feature bits
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

// virtqueue descriptor flags
const VIRTQ_DESC_F_NEXT:  u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

const QUEUE_SIZE: usize = 16;

/// Virtqueue layout: two 4096-byte pages.
/// Page 0 (offset 0): descriptor table + available ring + padding.
/// Page 1 (offset 4096): used ring + padding.
/// The device is given the physical address of page 0 as the queue PFN.
/// It computes the used ring at align(desc_size + avail_size, 4096) = 4096.
#[repr(C, align(4096))]
pub struct Virtqueue {
    // Page 0: descriptor table (256 bytes) + available ring (4 + 32 bytes) + padding
    desc:        [VirtqDesc; QUEUE_SIZE],   // 256 bytes at offset 0
    avail_flags: u16,
    avail_idx:   u16,
    avail_ring:  [u16; QUEUE_SIZE],         // 32 bytes
    _pad0:       [u8; 4096 - 256 - 4 - 32],
    // Page 1 (at offset 4096): used ring
    used_flags:  u16,
    used_idx:    u16,
    used_ring:   [VirtqUsedElem; QUEUE_SIZE], // 128 bytes
    _pad1:       [u8; 4096 - 4 - 128],
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct VirtqDesc {
    addr:  u64,
    len:   u32,
    flags: u16,
    next:  u16,
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct VirtqUsedElem {
    id:  u32,
    len: u32,
}

/// virtio-net packet header (legacy, no checksum)
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct VirtioNetHdr {
    flags:       u8,
    gso_type:    u8,
    hdr_len:     u16,
    gso_size:    u16,
    csum_start:  u16,
    csum_offset: u16,
}

const HDR_LEN: usize = core::mem::size_of::<VirtioNetHdr>();
const PKT_SIZE: usize = 1514;
const BUF_SIZE: usize = PKT_SIZE + HDR_LEN;

pub struct VirtioNet {
    pub mac:  [u8; 6],
    io_base:  u16,
    phys_offset: u64,
    // RX queue (index 0)
    rx_queue:     &'static mut Virtqueue,
    // Bug 2 fix: Box so heap address is stable even when struct moves
    rx_bufs:      alloc::boxed::Box<[[u8; BUF_SIZE]; QUEUE_SIZE]>,
    rx_last_used: u16,
    // TX queue (index 1)
    tx_queue: &'static mut Virtqueue,
    tx_buf:   [u8; BUF_SIZE],
}

impl VirtioNet {
    /// Scan PCI bus for virtio-net device (vendor 0x1AF4, device 0x1000).
    // Bug 1 fix: remove _ prefix so phys_offset is used
    pub fn probe(phys_offset: u64) -> Option<Self> {
        for bus in 0u8..=255 {
            for dev in 0u8..32 {
                let val = pci_read32(bus, dev, 0, 0);
                if val == 0xFFFF_FFFF { continue; }
                let vendor = (val & 0xFFFF) as u16;
                let device = (val >> 16) as u16;
                if vendor == 0x1AF4 && device == 0x1000 {
                    // Bug 1 fix: pass phys_offset to init
                    return Self::init(bus, dev, phys_offset);
                }
            }
        }
        None
    }

    // Bug 1 fix: accept phys_offset parameter
    fn init(bus: u8, dev: u8, phys_offset: u64) -> Option<Self> {
        // Enable bus master + I/O space access in command register (offset 0x04)
        let cmd = pci_read32(bus, dev, 0, 0x04);
        pci_write32(bus, dev, 0, 0x04, cmd | 0x5); // I/O enable (bit 0) + bus master (bit 2)

        // BAR0 is I/O space for legacy virtio; mask off lower 2 bits (I/O indicator)
        let bar0 = pci_read32(bus, dev, 0, 0x10) & !0x3;
        // Bug 3 fix: check BAR0 fits in u16 before casting
        if bar0 > 0xFFFF {
            crate::serial_println!("virtio-net: BAR0 {:#x} too large for u16", bar0);
            return None;
        }
        let io_base = bar0 as u16;

        // Virtio device initialisation sequence
        unsafe {
            // 1. Reset device
            PortWriteOnly::<u8>::new(io_base + VIRTIO_PCI_STATUS).write(0);
            // 2. Acknowledge + Driver
            PortWriteOnly::<u8>::new(io_base + VIRTIO_PCI_STATUS)
                .write(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
        }

        // Bug 5 fix: Read MAC address AFTER writing ACKNOWLEDGE | DRIVER status
        let mut mac = [0u8; 6];
        for i in 0..6 {
            mac[i] = unsafe {
                PortReadOnly::<u8>::new(io_base + VIRTIO_NET_CONFIG_MAC + i as u16).read()
            };
        }

        unsafe {
            // 3. Negotiate features: request MAC feature only
            let host_features = PortReadOnly::<u32>::new(io_base + VIRTIO_PCI_HOST_FEATURES).read();
            let guest_features = host_features & VIRTIO_NET_F_MAC;
            Port::<u32>::new(io_base + VIRTIO_PCI_GUEST_FEATURES).write(guest_features);
            // 4. Features OK (legacy devices may not support this bit but it's harmless)
            Port::<u8>::new(io_base + VIRTIO_PCI_STATUS)
                .write(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);
        }

        // Allocate virtqueues (8192-byte, 4096-aligned for two-page layout)
        let rx_queue: &'static mut Virtqueue = alloc_virtqueue();
        let tx_queue: &'static mut Virtqueue = alloc_virtqueue();

        // Bug 1 fix: convert virtual queue addresses to physical for DMA
        let rx_phys = (rx_queue as *mut Virtqueue as u64) - phys_offset;
        let tx_phys = (tx_queue as *mut Virtqueue as u64) - phys_offset;

        // Bug 2 fix: allocate rx_bufs on heap so addresses stay stable after struct moves
        let mut rx_bufs: alloc::boxed::Box<[[u8; BUF_SIZE]; QUEUE_SIZE]> =
            alloc::boxed::Box::new([[0u8; BUF_SIZE]; QUEUE_SIZE]);

        // Set up RX queue (queue index 0) - must happen before building descriptors
        // setup_queue needs io_base and phys_offset; call helper directly here
        unsafe {
            Port::<u16>::new(io_base + VIRTIO_PCI_QUEUE_SELECT).write(0);
            // Bug 1 fix: use physical address / 4096 for PFN
            Port::<u32>::new(io_base + VIRTIO_PCI_QUEUE_ADDR)
                .write((rx_phys / 4096) as u32);
        }

        // Fill all RX descriptors with receive buffers and add them to available ring
        // Bug 2 fix: use heap addresses from the Box (stable, won't move)
        // Bug 1 fix: subtract phys_offset for DMA addresses
        for i in 0..QUEUE_SIZE {
            let buf_ptr = rx_bufs[i].as_mut_ptr() as u64;
            rx_queue.desc[i] = VirtqDesc {
                addr:  buf_ptr - phys_offset,
                len:   BUF_SIZE as u32,
                flags: VIRTQ_DESC_F_WRITE,
                next:  0,
            };
            rx_queue.avail_ring[i] = i as u16;
        }
        rx_queue.avail_idx = QUEUE_SIZE as u16;

        // Set up TX queue (queue index 1)
        unsafe {
            Port::<u16>::new(io_base + VIRTIO_PCI_QUEUE_SELECT).write(1);
            // Bug 1 fix: use physical address / 4096 for PFN
            Port::<u32>::new(io_base + VIRTIO_PCI_QUEUE_ADDR)
                .write((tx_phys / 4096) as u32);
        }

        // Bug 1 fix: store phys_offset in struct
        let vdev = VirtioNet {
            mac,
            io_base,
            phys_offset,
            rx_queue,
            rx_bufs,
            rx_last_used: 0,
            tx_queue,
            tx_buf: [0u8; BUF_SIZE],
        };

        // Signal driver ready
        unsafe {
            Port::<u8>::new(vdev.io_base + VIRTIO_PCI_STATUS).write(
                VIRTIO_STATUS_ACKNOWLEDGE
                    | VIRTIO_STATUS_DRIVER
                    | VIRTIO_STATUS_FEATURES_OK
                    | VIRTIO_STATUS_DRIVER_OK,
            );
        }

        Some(vdev)
    }

    fn setup_queue(&mut self, queue_idx: u16, phys_addr: u64) {
        unsafe {
            Port::<u16>::new(self.io_base + VIRTIO_PCI_QUEUE_SELECT).write(queue_idx);
            // Write queue PFN = physical address divided by 4096
            Port::<u32>::new(self.io_base + VIRTIO_PCI_QUEUE_ADDR)
                .write((phys_addr / 4096) as u32);
        }
    }

    fn notify(&mut self, queue: u16) {
        unsafe {
            Port::<u16>::new(self.io_base + VIRTIO_PCI_QUEUE_NOTIFY).write(queue);
        }
    }
}

/// Allocate one Virtqueue (8192 bytes, 4096-byte aligned) from the heap and leak it.
fn alloc_virtqueue() -> &'static mut Virtqueue {
    use alloc::alloc::Layout;
    // Virtqueue is 8192 bytes (two 4096-byte pages), aligned to 4096
    let layout = Layout::from_size_align(8192, 4096).unwrap();
    let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        panic!("virtio queue allocation failed");
    }
    unsafe { &mut *(ptr as *mut Virtqueue) }
}

// ─── smoltcp Device trait ───────────────────────────────────────────────────

/// RxToken owns the packet data as a Vec to avoid self-referential borrow issues.
pub struct VirtioRxToken {
    packet: Vec<u8>,
}

pub struct VirtioTxToken<'a> {
    device: &'a mut VirtioNet,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut pkt = self.packet;
        f(&mut pkt)
    }
}

impl<'a> TxToken for VirtioTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // Zero the virtio header prefix
        for b in self.device.tx_buf[..HDR_LEN].iter_mut() {
            *b = 0;
        }
        // Let smoltcp write the Ethernet frame into the payload region
        let result = f(&mut self.device.tx_buf[HDR_LEN..HDR_LEN + len]);

        // Build TX descriptor for the whole buffer (header + frame)
        // Bug 1 fix: subtract phys_offset for DMA address
        let tx_buf_ptr = (self.device.tx_buf.as_ptr() as u64) - self.device.phys_offset;
        let total_len  = (HDR_LEN + len) as u32;
        self.device.tx_queue.desc[0] = VirtqDesc {
            addr:  tx_buf_ptr,
            len:   total_len,
            flags: 0,
            next:  0,
        };
        // Add descriptor 0 to the available ring
        let avail_idx = self.device.tx_queue.avail_idx;
        self.device.tx_queue.avail_ring[(avail_idx % QUEUE_SIZE as u16) as usize] = 0;
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.tx_queue.avail_idx = avail_idx.wrapping_add(1);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(1);

        result
    }
}

impl Device for VirtioNet {
    type RxToken<'a> = VirtioRxToken where Self: 'a;
    type TxToken<'a> = VirtioTxToken<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let used_idx = self.rx_queue.used_idx;
        if self.rx_last_used == used_idx {
            return None;
        }
        let slot = (self.rx_last_used % QUEUE_SIZE as u16) as usize;
        let used_elem = self.rx_queue.used_ring[slot];
        self.rx_last_used = self.rx_last_used.wrapping_add(1);

        let desc_idx  = used_elem.id as usize;
        let pkt_len   = used_elem.len as usize;

        // Bug 4 fix: guard against out-of-bounds desc_idx and cap payload_len
        if desc_idx >= QUEUE_SIZE { return None; }
        let payload_len = pkt_len.saturating_sub(HDR_LEN).min(PKT_SIZE);

        // Copy packet payload into an owned Vec before any further borrow of self
        let packet = Vec::from(&self.rx_bufs[desc_idx][HDR_LEN..HDR_LEN + payload_len]);

        // Recycle this descriptor back to the available ring
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let avail_idx = self.rx_queue.avail_idx;
        self.rx_queue.avail_ring[(avail_idx % QUEUE_SIZE as u16) as usize] = desc_idx as u16;
        self.rx_queue.avail_idx = avail_idx.wrapping_add(1);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.notify(0);

        Some((VirtioRxToken { packet }, VirtioTxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(VirtioTxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps.max_burst_size = Some(1);
        caps
    }
}
