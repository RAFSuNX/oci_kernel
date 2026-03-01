#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod serial;
mod gdt;
mod interrupts;
mod memory;

use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

entry_point!(kernel_main);

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
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // force-unlock in case panic fired while serial lock was held
    unsafe { serial::SERIAL.force_unlock() };
    serial_println!("KERNEL PANIC: {}", info);
    loop {}
}
