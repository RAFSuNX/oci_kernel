use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use pic8259::ChainedPics;
use spin::Lazy;
use crate::{serial_println, gdt};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> = spin::Mutex::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer    = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    // CPU exceptions
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault.set_handler_fn(gpf_handler);

    // Hardware IRQs
    idt[InterruptIndex::Timer as u8].set_handler_fn(timer_handler);
    idt[InterruptIndex::Keyboard as u8].set_handler_fn(keyboard_handler);

    idt
});

pub fn init() {
    IDT.load();
    unsafe { PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(frame: InterruptStackFrame) {
    serial_println!("EXCEPTION: BREAKPOINT\n{:#?}", frame);
}

extern "x86-interrupt" fn double_fault_handler(
    frame: InterruptStackFrame, _code: u64) -> ! {
    panic!("DOUBLE FAULT\n{:#?}", frame);
}

extern "x86-interrupt" fn page_fault_handler(
    frame: InterruptStackFrame, error: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    serial_println!("PAGE FAULT: {:?} at {:?}", error, Cr2::read());
    serial_println!("{:#?}", frame);
    loop {}
}

extern "x86-interrupt" fn gpf_handler(frame: InterruptStackFrame, code: u64) {
    serial_println!("GENERAL PROTECTION FAULT: code={}", code);
    serial_println!("{:#?}", frame);
    loop {}
}

extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer as u8) };
}

extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port: Port<u8> = Port::new(0x60);
    let _scancode: u8 = unsafe { port.read() };
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard as u8) };
}
