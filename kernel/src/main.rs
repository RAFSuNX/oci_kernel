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
use bootloader_api::{BootInfo, BootloaderConfig, entry_point, config::Mapping};
#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use linked_list_allocator::LockedHeap;

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Tell the bootloader to map all of physical RAM at a fixed offset.
/// Without this, `boot_info.physical_memory_offset` is `None` and memory
/// initialisation will panic.
#[cfg(not(test))]
const BOOT_CONFIG: BootloaderConfig = {
    let mut cfg = BootloaderConfig::new_default();
    cfg.mappings.physical_memory = Some(Mapping::Dynamic);
    cfg
};

#[cfg(not(test))]
entry_point!(kernel_main, config = &BOOT_CONFIG);

/// Default embedded boot configuration.
///
/// host: 8080 → QEMU forwards localhost:8080 → kernel:80 → nginx container:80.
/// Port 8080 avoids needing root on the Linux host for ports < 1024.
#[cfg(not(test))]
const DEFAULT_CONFIG_YAML: &str = "\
containers:
  - image: nginx:latest
    ports:
      - host: 8080
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

    // ── 7. Start declared containers + register port forwards ────────────────
    for spec in boot_cfg.containers {
        serial_println!("  Starting container: {} ...", spec.image);
        let image_name  = spec.image.clone();
        let port_list   = spec.ports.clone(); // save before spec is moved
        let mut c = container::runtime::Container::create(spec);
        match c.start() {
            Ok(()) => {
                serial_println!("  [OK] {} running (PID ns isolated)", image_name);
                let cid = c.id;
                container::STORE.lock().register(
                    cid,
                    image_name,
                    container::runtime::ContainerState::Running,
                );
                // Register each port mapping so the shell and kernel info show it.
                {
                    use container::spec::ActivePortForward;
                    let mut pf = container::PORT_FORWARDS.lock();
                    for pm in &port_list {
                        pf.push(ActivePortForward {
                            container_id:   cid.0,
                            host_port:      pm.host,
                            container_port: pm.container,
                        });
                        serial_println!(
                            "  [OK] Port forward: host:{} -> container:{}",
                            pm.host, pm.container
                        );
                    }
                }
            }
            Err(e) => serial_println!("  [ERR] {}: {}", image_name, e),
        }
    }

    // ── 8. HTTP listener ──────────────────────────────────────────────────────
    // Open a smoltcp TCP socket on port 80 (the container's port).
    // QEMU forwards host:8080 → kernel:80 via hostfwd (see Makefile).
    // While the operator is at the shell, the serial read loop polls the
    // network so HTTP requests are served concurrently.
    net::setup_http_listener(80);

    serial_println!("OCI Kernel ready.  Type 'help' on the serial console.");
    serial_println!("HTTP: curl http://localhost:8080  (QEMU) or <machine-ip>:8080 (real HW)");

    // ── 9. Serial getty ───────────────────────────────────────────────────────
    // `Getty::run()` is diverging — it loops forever reading from COM1.
    // The serial read loop calls `net::serve_http_once()` between keystrokes
    // so HTTP requests are handled while the operator is idle.
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
