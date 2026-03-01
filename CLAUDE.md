# OCI Kernel — CLAUDE.md

## Project Vision

A Rust x86_64 monolithic kernel where the **OCI container runtime is a first-class primitive**.
No runc, no containerd daemon, no Linux compatibility layer. The kernel IS the container runtime.

**Milestone 1:** Boot QEMU → pull `nginx:latest` from Docker Hub → run isolated → serve HTTP on port 80.

---

## Toolchain Requirements

- **Rust nightly** is required — uses `abi_x86_interrupt` feature and `build-std`
- `rustup target add x86_64-unknown-none` (for dependencies)
- `cargo install bootimage`
- `llvm-tools-preview` component: `rustup component add llvm-tools-preview`
- QEMU: `qemu-system-x86_64` with virtio-net support
- Linker: `rust-lld` (part of llvm-tools-preview)

---

## Build Commands

```bash
make build    # compile kernel binary
make image    # create bootable disk image (oci-kernel.img)
make qemu     # build + run in QEMU with serial on stdout
make debug    # build + run with GDB stub on :1234 (paused at boot)
make clean    # clean build artifacts and image
```

All commands run from the workspace root (`/home/rafsunx/claude/side_project/OS`).

---

## Project Structure

```
OS/
├── CLAUDE.md               ← this file
├── Cargo.toml              ← workspace root (profile tables live here)
├── Makefile
├── .cargo/config.toml      ← build-std, json-target-spec, default target
├── docs/
│   └── plans/
│       ├── 2026-03-01-oci-kernel-design.md
│       └── 2026-03-01-oci-kernel-impl.md   ← 18-task implementation plan
└── kernel/
    ├── Cargo.toml
    ├── x86_64-oci-kernel.json  ← custom target spec
    └── src/
        ├── main.rs             ← kernel entry, global allocator, panic handler
        ├── serial.rs           ← UART serial (COM1), serial_print!/serial_println!
        ├── gdt.rs              ← GDT + TSS (double fault IST stack 20KB)
        ├── interrupts.rs       ← IDT, PIC8259 (IRQ 0-15 → vectors 32-47)
        └── memory/
            ├── mod.rs          ← BootInfoFrameAllocator, init_mapper, memory::init
            ├── buddy.rs        ← BuddyAllocator (physical frame management)
            └── heap.rs         ← kernel heap: 8MB at 0xFFFF_C000_0000_0000
```

---

## Key Technical Decisions

### Custom Target
`kernel/x86_64-oci-kernel.json` — bare metal x86_64:
- No SSE/MMX (`-mmx,-sse,+soft-float`) with `rustc-abi: "x86-softfloat"`
- Linker: `rust-lld` via `ld.lld` flavor
- `disable-redzone: true`, `panic-strategy: abort`

### Build Config (`.cargo/config.toml`)
- `build-std = ["core", "compiler_builtins", "alloc"]`
- `json-target-spec = true` — **required**, do not remove, it enables JSON custom target loading
- `build-std-features = ["compiler-builtins-mem"]` — provides memcpy/memset in no_std

### Memory Layout
- Heap: `0xFFFF_C000_0000_0000` → `+8MB` (kernel virtual space)
- Physical memory mapped at offset provided by bootloader (passed via `BootInfo`)

### Allocators
- **BootInfoFrameAllocator** — walks bootloader memory map, serves physical frames at boot
- **BuddyAllocator** (`memory/buddy.rs`) — O(1) alloc/free, 11 orders (1–1024 frames), coalescing
- **LockedHeap** (linked_list_allocator) — Rust global allocator backed by the mapped heap pages

### Interrupts
- PIC1 offset: 32, PIC2 offset: 40
- Always wrap `PICS.lock()` calls in `without_interrupts` to prevent deadlock
- Double fault handler uses IST[0] (separate stack in TSS)

### Panic Handler
- Must call `unsafe { serial::SERIAL.force_unlock() }` before printing — prevents deadlock if panic fires while serial lock is held

### Dependencies (no_std safe)
| Purpose | Crate |
|---|---|
| Networking | `smoltcp 0.11` (no default features, `proto-ipv4,socket-tcp,socket-udp,medium-ethernet,alloc`) |
| JSON parsing | `serde_json 1` (no default features, `alloc`) |
| gzip decompression | `miniz_oxide 0.8` (no default features, `with-alloc`) — **not flate2** |
| Sync primitives | `spin 0.9` with `features = ["lazy"]` |
| Boot interface | `bootloader_api 0.11` — separate crate from `bootloader` |

---

## Critical Gotchas (Learned Lessons)

1. **`bootloader` ≠ `bootloader_api`** — The `0.11` series uses `bootloader_api` as a separate dependency crate. Using `bootloader` with `map_physical_memory` feature does not exist in 0.11.

2. **`flate2` is not no_std** — Use `miniz_oxide` with `with-alloc` feature instead.

3. **`json-target-spec = true` is required** — Despite some tools reporting it as invalid, removing it breaks custom JSON target loading. Keep it.

4. **`rustc-abi: "x86-softfloat"`** — Required when using `+soft-float` CPU feature. The value is `"x86-softfloat"` not `"softfloat"`.

5. **Profile tables belong in workspace root** — Putting `[profile.*]` in `kernel/Cargo.toml` causes Cargo warnings. Keep them in `/Cargo.toml`.

6. **No `lib.rs` with `[[bin]]`** — Having both causes build-std conflicts for the host target. Use only `main.rs` with `entry_point!` macro.

7. **`spin::Lazy` requires feature flag** — `spin = { version = "0.9", features = ["lazy"] }`.

---

## Container Semantics (Design Rules)

- **upper/** (writable CoW layer) → deleted on container stop
- **logs/** → persisted after container stop
- **volumes/** → persisted always
- Host filesystem: **never visible** unless explicitly mounted
- Host mounts: **read-only by default** unless `access: readwrite` declared
- No disk quota on upper/ by default; user can declare with dedicated path/volume

---

## Git Conventions

- **No `Co-Authored-By:` tags** in commits
- Commit messages: imperative mood (`feat:`, `fix:`, `refactor:`)
- Frequent small commits, one logical change per commit

---

## Implementation Plan Status

See `docs/plans/2026-03-01-oci-kernel-impl.md` for the 18-task plan.

| Phase | Tasks | Status |
|---|---|---|
| Phase 1: Scaffold | 1–3 | ✅ Complete |
| Phase 2: CPU Primitives | 4–5 | ✅ Complete |
| Phase 3: Memory | 6 | ⚠️ In progress (fixes pending) |
| Phase 4: Scheduler | 7 | Pending |
| Phase 5: Networking | 8–9 | Pending |
| Phase 6: OCI Pull | 10–11 | Pending |
| Phase 7: Filesystem | 12 | Pending |
| Phase 8: Isolation | 13 | Pending |
| Phase 9: Container Lifecycle | 14–15 | Pending |
| Phase 10: Operator Access | 16 | Pending |
| Phase 11: Config + Boot | 17–18 | Pending |
