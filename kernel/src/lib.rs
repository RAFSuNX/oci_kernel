// This lib.rs exists solely so that `cargo test --lib --target x86_64-unknown-linux-gnu`
// can compile and run the unit tests embedded in the kernel modules.
// When compiled for the bare-metal target, no_std is required.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod memory;
