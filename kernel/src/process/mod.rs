extern crate alloc;
use alloc::vec::Vec;
use x86_64::VirtAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Running,
    Ready,
    Blocked,
    Zombie,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CpuContext {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8:  u64, pub r9:  u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub rflags: u64,
}

pub struct Process {
    pub id:           ProcessId,
    pub state:        ProcessState,
    pub context:      CpuContext,
    pub kernel_stack: VirtAddr,
    pub page_table:   u64,       // Cr3 value
    pub exit_code:    Option<i32>,
}

pub mod scheduler;
