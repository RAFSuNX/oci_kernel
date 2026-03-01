#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod serial;

use bootloader_api::{BootInfo, entry_point};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static mut BootInfo) -> ! {
    serial::init();
    serial_println!("OCI Kernel 0.1.0 booting...");
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // force-unlock in case panic fired while serial lock was held
    unsafe { serial::SERIAL.force_unlock() };
    serial_println!("KERNEL PANIC: {}", info);
    loop {}
}
