// In test mode we compile for the host (x86_64-unknown-linux-gnu) so the
// std test harness can run unit tests in the oci/ and fs/ modules.
// In production we are bare-metal no_std.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), feature(abi_x86_interrupt))]

// Hardware-specific modules are only compiled for the real kernel, not
// when running host-side unit tests.
#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))] mod serial;
#[cfg(not(test))] mod gdt;
#[cfg(not(test))] mod interrupts;
#[cfg(not(test))] mod memory;
#[cfg(not(test))] mod process;
#[cfg(not(test))] mod net;

// Pure-logic modules are always compiled (they have unit tests).
mod oci;
mod fs;
mod isolation;
mod container;

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

#[cfg(not(test))]
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::init();
    serial_println!("OCI Kernel 0.1.0 booting...");
    gdt::init();              serial_println!("[OK] GDT");
    interrupts::init();       serial_println!("[OK] IDT + PIC");
    memory::init(boot_info);  serial_println!("[OK] Memory");
    // smoke test: heap works
    {
        use alloc::vec;
        let v = vec![1u64, 2, 3, 42];
        serial_println!("[OK] Heap: {:?}", v);
    }
    let phys_offset = boot_info.physical_memory_offset.into_option()
        .expect("physical memory offset not provided");
    net::init(phys_offset);
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // force-unlock in case panic fired while serial lock was held
    unsafe { serial::SERIAL.force_unlock() };
    serial_println!("KERNEL PANIC: {}", info);
    loop {}
}
