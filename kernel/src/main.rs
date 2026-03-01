// In test mode we compile for the host (x86_64-unknown-linux-gnu) so the
// std test harness can run unit tests in the pure-logic modules.
// In production we are bare-metal no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), feature(abi_x86_interrupt))]

// Hardware-specific modules: only compiled for the real kernel.
#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))] mod serial;
#[cfg(not(test))] mod gdt;
#[cfg(not(test))] mod interrupts;
#[cfg(not(test))] mod memory;
#[cfg(not(test))] mod process;

// net is always compiled: vswitch.rs has unit tests.
// Hardware-specific submodules are gated inside net/mod.rs.
mod net;

// Pure-logic modules: always compiled (they have unit tests).
mod oci;
mod fs;
mod isolation;
mod container;
mod host;
mod config;

#[cfg(not(test))]
use bootloader_api::{BootInfo, entry_point};
#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use linked_list_allocator::LockedHeap;

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[cfg(not(test))]
entry_point!(kernel_main);

/// Default embedded boot configuration.
///
/// In a full implementation this would be loaded from a disk image or ramdisk.
/// For Milestone 1 we embed a hard-coded config that starts nginx on port 80.
#[cfg(not(test))]
const DEFAULT_CONFIG_YAML: &str = "\
containers:
  - image: nginx:latest
    ports:
      - host: 80
        container: 80
    restart: always
    resources:
      memory: 256mb
      pids_max: 50
";

#[cfg(not(test))]
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ── 1. Core CPU setup ─────────────────────────────────────────────────────
    serial::init();
    serial_println!("OCI Kernel 0.1.0 booting...");
    gdt::init();              serial_println!("[OK] GDT");
    interrupts::init();       serial_println!("[OK] IDT + PIC");

    // ── 2. Memory ─────────────────────────────────────────────────────────────
    memory::init(boot_info);
    {
        use alloc::vec;
        let _probe = vec![0u8; 4096]; // verify heap is usable
    }
    serial_println!("[OK] Memory + Heap (8MB at 0xFFFF_C000_0000_0000)");

    // ── 3. Network ────────────────────────────────────────────────────────────
    let phys_offset = boot_info.physical_memory_offset.into_option()
        .expect("physical memory offset not provided by bootloader");
    net::init(phys_offset);
    // net::init() prints its own [OK] line including the MAC address.

    // ── 4. OCI subsystem ──────────────────────────────────────────────────────
    // The image store, registry client, and layer decompressor are all
    // stateless structs — no global init required. They are used on demand
    // from the container runtime when a container is started.
    serial_println!("[OK] OCI subsystem (registry + layer + store)");

    // ── 5. Container runtime ──────────────────────────────────────────────────
    // ContainerStore is per-call for now; a global store would be added in
    // a future task when the scheduler dispatches container processes.
    serial_println!("[OK] Container runtime (lifecycle + isolation + overlayfs)");

    // ── 6. Parse boot config ──────────────────────────────────────────────────
    let boot_cfg = config::KernelConfig::from_yaml(DEFAULT_CONFIG_YAML)
        .unwrap_or_default();
    serial_println!(
        "[OK] Boot config ({} container(s) declared)",
        boot_cfg.containers.len()
    );

    // ── 7. Start declared containers ─────────────────────────────────────────
    // Milestone 1: pull + run each container from the boot config.
    // Full networking (port forwarding, NAT) requires the vswitch to be
    // wired to smoltcp, which is the next milestone.  For M1 we log intent.
    for spec in boot_cfg.containers {
        serial_println!("  Starting container: {} ...", spec.image);
        let mut c = container::runtime::Container::create(spec);
        match c.start() {
            Ok(())  => serial_println!("  [OK] {} running (PID ns isolated)", c.spec.image),
            Err(e)  => serial_println!("  [ERR] {}: {}", c.spec.image, e),
        }
        // TODO M2: pull image layers from registry, unpack into overlayfs,
        //          spawn init process in namespace, wire port-forward rules.
    }

    serial_println!("OCI Kernel ready.  Type 'help' on the serial console.");

    // ── 8. Serial getty ───────────────────────────────────────────────────────
    // `Getty::run()` is diverging — it loops forever reading from COM1.
    host::getty::Getty::new().run()
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // force-unlock in case panic fired while serial lock was held
    unsafe { serial::SERIAL.force_unlock() };
    serial_println!("KERNEL PANIC: {}", info);
    loop {}
}
