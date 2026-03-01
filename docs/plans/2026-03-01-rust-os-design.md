# Rust OS Design Document
**Date:** 2026-03-01
**Target:** x86_64 monolithic kernel written in Rust
**Milestone:** Boot and run an interactive shell

---

## 1. Goals & Constraints

- **Practical OS** — not a toy, designed to run real userspace programs
- **x86_64** target architecture
- **Monolithic kernel** — all core services in kernel space
- **Custom syscall interface** — clean Rust-idiomatic ABI first, POSIX compatibility layer later
- **First milestone:** kernel boots, loads a shell binary, shell accepts and executes commands

---

## 2. Project Structure

```
rust-os/
├── kernel/
│   ├── src/
│   │   ├── main.rs          # Kernel entry point (kernel_main)
│   │   ├── memory/          # Physical allocator + virtual memory
│   │   ├── interrupts/      # IDT, exception handlers, IRQs
│   │   ├── process/         # Process table, scheduler, context switch
│   │   ├── syscall/         # Syscall dispatcher and handlers
│   │   ├── drivers/         # VGA, serial, keyboard, PIT timer
│   │   └── fs/              # Virtual filesystem + ramfs
│   └── Cargo.toml
├── userspace/
│   ├── shell/               # Shell binary (target milestone)
│   └── libc/                # Minimal custom libc (syscall wrappers)
├── .cargo/
│   └── config.toml          # Target triple + linker config
└── Makefile                 # build, qemu, debug targets
```

### Key crates
| Crate | Purpose |
|---|---|
| `bootloader` | BIOS/UEFI boot, memory map handoff to kernel |
| `x86_64` | Safe wrappers for page tables, GDT, IDT, CPU instructions |
| `pic8259` | Programmable Interrupt Controller driver |
| `uart_16550` | Serial port (COM1) for debug output |
| `spin` | Spinlock primitives (no OS-level locks in kernel) |
| `linked_list_allocator` | Heap allocator — enables Box, Vec, String in kernel |

---

## 3. Boot Process

```
BIOS/UEFI
  → bootloader crate (long mode setup, page tables, framebuffer)
  → kernel_main(boot_info: &BootInfo)
      → init GDT
      → init IDT
      → init physical memory allocator
      → init virtual memory + kernel heap
      → init drivers (serial, VGA, keyboard, PIT)
      → init scheduler
      → load shell ELF from ramfs
      → start scheduling loop
```

---

## 4. Memory Layout

```
0x0000_0000_0000_0000  →  user space (low 128 TB)
0xFFFF_8000_0000_0000  →  physical memory map (identity mapped)
0xFFFF_C000_0000_0000  →  kernel heap
0xFFFF_FFFF_8000_0000  →  kernel code/data (.text, .rodata, .bss)
```

- **Physical allocator:** Bitmap allocator over `bootloader`-provided memory map. 4KB frame granularity.
- **Virtual memory:** Each process owns a separate Level-4 page table (own `Cr3`). Kernel is mapped into the upper half of every process address space — avoids TLB flush on syscall entry.
- **Heap:** `linked_list_allocator` as `#[global_allocator]` for kernel dynamic allocations.

---

## 5. Interrupts & Drivers

### IDT layout
```
0–31    CPU exceptions (Page Fault, GP Fault, Double Fault, ...)
32–47   Hardware IRQs via PIC8259
  IRQ0  PIT Timer       → scheduler tick (100Hz)
  IRQ1  PS/2 Keyboard   → input buffer
  IRQ4  Serial COM1     → debug
0x80    Syscall vector
```

### Drivers for milestone 1
| Driver | Purpose | Approach |
|---|---|---|
| Serial UART | Boot debug output | `uart_16550` crate |
| VGA text mode | Shell display | Write to `0xb8000` directly |
| PS/2 Keyboard | Shell input | IRQ1 + scancode-to-ASCII table |
| PIT Timer | Scheduler ticks | IRQ0 + `pic8259` |

### Input pipeline
```
PS/2 IRQ fires
  → ISR reads scancode from port 0x60
  → translates to ASCII / key event
  → pushes into per-process stdin ring buffer
  → wakes any process blocked on read()
```

---

## 6. Process & Scheduler

### Process struct
```rust
struct Process {
    pid: u64,
    state: ProcessState,    // Running, Ready, Blocked, Zombie
    page_table: PhysFrame,  // Cr3 value (own address space)
    kernel_stack: VirtAddr, // Stack used during syscalls/interrupts
    context: CpuContext,    // Saved registers
    stdin: RingBuffer,
    stdout: RingBuffer,
}
```

### Scheduler — Round Robin
```
Timer IRQ fires (every 10ms)
  → save current process CpuContext
  → pick next Ready process from queue
  → restore its CpuContext
  → switch Cr3
  → iretq into userspace
```

### Context switch
Each process has two stacks:
- **Userspace stack** — in its own address space (ring 3)
- **Kernel stack** — in kernel space, switched to automatically on interrupt/syscall (ring 0)

Context = all general-purpose registers + rip + rflags + rsp, saved to process struct on preemption.

---

## 7. Syscall Interface

**Mechanism:** `int 0x80` or `syscall` instruction. Syscall number in `rax`, result returned in `rax`.

### Initial syscall table
| # | Name | Signature | Purpose |
|---|---|---|---|
| 0 | `exit` | `(code: i32)` | Terminate process |
| 1 | `write` | `(fd: usize, buf: *const u8, len: usize) → isize` | Write to fd |
| 2 | `read` | `(fd: usize, buf: *mut u8, len: usize) → isize` | Read from fd |
| 3 | `spawn` | `(path: *const u8) → u64` | Create & run process, return pid |
| 4 | `wait` | `(pid: u64) → i32` | Wait for child, return exit code |
| 5 | `getpid` | `() → u64` | Return current PID |

---

## 8. Filesystem (Milestone 1: ramfs)

No disk driver needed for milestone 1. Shell ELF binary is embedded directly into the kernel image at compile time:

```rust
static SHELL_ELF: &[u8] = include_bytes!("../../userspace/shell/shell.elf");
```

A minimal ramfs maps path strings to byte slices. `spawn("/bin/shell")` looks up the path in ramfs, parses the ELF, maps segments into a new process address space, and schedules it.

---

## 9. Shell (Milestone 1 target)

Minimal shell behavior:
```
loop {
    print("$ ")
    line = read_line(stdin)
    parse command + args
    pid = spawn(command_path)
    wait(pid)
}
```

The shell is a userspace binary compiled separately with a custom target (`x86_64-unknown-none`). It communicates with the kernel exclusively through the syscall interface above.

---

## 10. Build System

```makefile
build:     # cargo build kernel + userspace, link final image
qemu:      # run in QEMU with serial output to terminal
debug:     # run QEMU with GDB stub, attach debugger
iso:       # package bootable ISO for real hardware testing
```

QEMU invocation:
```sh
qemu-system-x86_64 \
  -drive format=raw,file=rust-os.img \
  -serial stdio \
  -m 256M \
  -no-reboot
```

---

## 11. Milestone 1 Definition of Done

- [ ] Kernel boots in QEMU without panic
- [ ] Serial debug output visible on boot
- [ ] VGA text mode shows kernel messages
- [ ] Keyboard input captured correctly
- [ ] Shell process loads from ramfs and executes in ring 3
- [ ] Shell prompt appears and accepts input
- [ ] Shell can spawn and wait for at least one child command (e.g. `echo`)
