# Linux ABI Compatibility Layer — M2 Design

**Date:** 2026-03-02
**Goal:** Run unmodified generic Docker containers (nginx:latest, redis, postgres, etc.) on the OCI Kernel by implementing a Linux syscall compatibility layer directly in the kernel.

---

## Approach

Implement ~100 Linux syscalls in the kernel so that standard glibc binaries with dynamic linking can execute in user space (ring 3). The kernel loads the ELF binary plus `ld-linux-x86-64.so.2` from the container rootfs, sets up the aux vector, and jumps to the dynamic linker. All syscalls are handled in kernel space with no hypervisor layer.

This preserves the "kernel IS the container runtime" thesis.

---

## Virtual Address Space Layout

```
0xFFFF_FFFF_FFFF_FFFF  ┐
                        │  kernel virtual space (existing)
0xFFFF_8000_0000_0000  ┘  mapped in every process CR3 (syscall handlers work without CR3 switch)

0x0000_7FFF_FFFF_FFFF  ┐
                        │  user stack (grows down from 0x7FFF_0000_0000)
  0x0000_7FFE_0000_0000 │  ld-linux.so.2 base (fixed: 0x7FFF_0000_0000 - size)
                        │  glibc + .so dependencies (placed by ld-linux)
                        │  heap (grows up from program break)
                        │  ELF PT_LOAD segments (at ELF-requested vaddr)
0x0000_0000_0040_0000  ┘  typical ELF entry (0x400000)
0x0000_0000_0000_0000     null page (unmapped, catches null dereferences)
```

Each process has its own L4 page table (CR3). The kernel maps itself into the top half of every process's address space so that SYSCALL/SYSRET do not need to switch CR3 for kernel data access.

---

## Components

### 1. User Address Space (`kernel/src/memory/user.rs`)

- `UserAddressSpace::new()` — allocates a new L4 table, copies kernel mappings into top half
- `map_range(vaddr, frames, flags)` — maps physical frames at a user virtual address with given page flags (user-accessible, executable/no-execute, writable/read-only)
- `alloc_user_pages(vaddr, count)` — allocates frames from buddy allocator, maps them
- `copy_on_write_fork()` — duplicates the address space: code/rodata pages shared read-only, data/stack pages marked CoW (write fault triggers real copy)

### 2. ELF64 Loader (`kernel/src/exec/elf.rs`)

- Parse ELF64 header: validate magic `\x7FELF`, class 64, machine x86_64
- Iterate `PT_LOAD` segments: allocate user pages, copy segment bytes, zero BSS
- Detect `PT_INTERP`: read interpreter path (e.g. `/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2`), load it from the container rootfs at base `0x7FFF_0000_0000`
- Build initial user stack:
  ```
  [rsp] argc
  [rsp+8..] argv ptrs (null terminated)
  envp ptrs (null terminated)
  aux vector:
    AT_PHDR   → address of program headers in memory
    AT_PHENT  → sizeof(ElfPhdr) = 56
    AT_PHNUM  → number of program headers
    AT_PAGESZ → 4096
    AT_BASE   → ld-linux load base
    AT_FLAGS  → 0
    AT_ENTRY  → binary entry point
    AT_UID/GID/EUID/EGID → 0 (root)
    AT_RANDOM → 16 random bytes (from RDRAND)
    AT_NULL   → terminator
  ```
- Return: `(entry_rip, initial_rsp)` — entry is ld-linux's `_start` for dynamic binaries

### 3. SYSCALL Entry (`kernel/src/exec/entry.asm`)

Set MSRs during kernel init:
- `STAR`: ring 0 CS = 0x08, ring 3 CS = 0x18 (with RPL=3 → 0x1B)
- `LSTAR`: address of `syscall_entry` assembly label
- `SFMASK`: IF bit — disables interrupts on syscall entry

Assembly trampoline (naked function, never returns normally):
```asm
syscall_entry:
    swapgs                    ; GS base now points to per-CPU kernel data
    mov  [gs:CPU_USER_RSP], rsp
    mov  rsp, [gs:CPU_KERN_RSP]
    push rcx                  ; user rip
    push r11                  ; user rflags
    push rbp
    push rbx / r12-r15        ; callee-saved
    mov  rdi, rax             ; arg0 = syscall number
    mov  rsi, rdi             ; (rdi/rsi/rdx/r10/r8/r9 are args 1-6 per Linux ABI)
    call syscall_dispatch
    pop  r15-r12 / rbx / rbp
    pop  r11 / pop rcx
    mov  rsp, [gs:CPU_USER_RSP]
    swapgs
    sysretq
```

### 4. Syscall Dispatcher (`kernel/src/exec/syscall.rs`)

Match on syscall number (Linux x86_64 ABI). ~100 handlers:

**Process:**
`exit(1)`, `exit_group(231)`, `getpid(39)`, `gettid(186)`, `getuid(102)`, `getgid(104)`, `geteuid(107)`, `getegid(108)`, `uname(63)`, `arch_prctl(158)`, `prctl(157)`, `set_tid_address(218)`, `set_robust_list(273)`, `prlimit64(302)`, `getrlimit(97)`, `setrlimit(160)`

**Memory:**
`brk(12)`, `mmap(9)`, `munmap(11)`, `mprotect(10)`, `mremap(25)`, `madvise(28)`

**Files:**
`read(0)`, `write(1)`, `open(2)`, `close(3)`, `fstat(5)`, `lstat(6)`, `stat(4)`, `openat(257)`, `readv(19)`, `writev(20)`, `lseek(8)`, `access(21)`, `faccessat(269)`, `getcwd(79)`, `chdir(80)`, `mkdir(83)`, `mkdirat(258)`, `unlink(87)`, `unlinkat(263)`, `rename(82)`, `renameat2(316)`, `getdents64(217)`, `readlink(89)`, `readlinkat(267)`, `dup(32)`, `dup2(33)`, `dup3(292)`, `fcntl(72)`, `ioctl(16)`, `sendfile(40)`, `pipe(22)`, `pipe2(293)`

**Network:**
`socket(41)`, `bind(49)`, `connect(42)`, `listen(50)`, `accept4(288)`, `getsockname(51)`, `getpeername(52)`, `setsockopt(54)`, `getsockopt(55)`, `sendto(44)`, `recvfrom(45)`, `sendmsg(46)`, `recvmsg(47)`, `sendmmsg(307)`, `recvmmsg(299)`, `shutdown(48)`

**Epoll/Poll:**
`epoll_create1(291)`, `epoll_ctl(233)`, `epoll_wait(232)`, `epoll_pwait(281)`, `poll(7)`, `ppoll(271)`, `select(23)`, `pselect6(270)`

**Signals:**
`rt_sigaction(13)`, `rt_sigprocmask(14)`, `rt_sigreturn(15)`, `sigaltstack(131)`, `kill(62)`, `tgkill(234)`

**Time:**
`clock_gettime(228)`, `clock_nanosleep(230)`, `gettimeofday(96)`, `nanosleep(35)`, `times(100)`, `setitimer(38)`, `timerfd_create(283)`, `timerfd_settime(286)`, `timerfd_gettime(287)`

**Processes:**
`clone(56)`, `fork(57)`, `vfork(58)`, `execve(59)`, `execveat(322)`, `wait4(61)`, `waitid(247)`

**Futex:**
`futex(202)` — `FUTEX_WAIT`, `FUTEX_WAKE`, `FUTEX_WAIT_BITSET`, `FUTEX_WAKE_BITSET`

**Misc:**
`eventfd2(290)`, `sysinfo(99)`, `getrandom(318)`, `umask(95)`, `chroot(161)`, `setsid(112)`, `setpgid(109)`, `getppid(110)`, `socketpair(53)`

### 5. Per-Process fd Table (`kernel/src/exec/fd_table.rs`)

```rust
const FD_MAX: usize = 1024;

enum FileDesc {
    File { vfs: VfsHandle, offset: u64, flags: u32 },
    Socket(smoltcp::SocketHandle),
    Epoll(EpollInst),
    EventFd { value: u64, flags: u32 },
    TimerFd { spec: ITimerSpec, armed: bool },
    Stdin, Stdout, Stderr,
}

struct FdTable {
    entries: [Option<FileDesc>; FD_MAX],
    cloexec: u64,   // bitmap for O_CLOEXEC
}
```

fd 0/1/2 pre-populated as Stdin/Stdout/Stderr (Stdout/Stderr → serial).

### 6. Virtual Filesystems (`kernel/src/exec/procfs.rs`, `devfs.rs`)

Paths resolved before hitting overlayfs:

| Path | Returns |
|------|---------|
| `/proc/sys/net/core/somaxconn` | `"128\n"` |
| `/proc/sys/net/ipv4/tcp_fin_timeout` | `"60\n"` |
| `/proc/cpuinfo` | single CPU entry |
| `/proc/meminfo` | basic memory stats |
| `/proc/self/maps` | current process's mapped regions |
| `/proc/self/status` | PID, UID, GID, etc. |
| `/proc/self/fd/` | symlinks to open fds |
| `/proc/<pid>/` | same for other processes |
| `/dev/null` | reads → 0 bytes, writes → discarded |
| `/dev/zero` | reads → zero bytes |
| `/dev/urandom` | reads → RDRAND output |
| `/etc/passwd` | `root:x:0:0:root:/root:/bin/sh\n` |
| `/etc/group` | `root:x:0:\n` |
| `/etc/hostname` | container hostname |
| `/etc/resolv.conf` | `nameserver 10.0.2.3\n` (QEMU DNS) |
| `/etc/hosts` | `127.0.0.1 localhost\n` |
| `/tmp/` | in-memory tmpfs backed by kernel heap |

### 7. Socket → smoltcp Bridge (`kernel/src/exec/socket.rs`)

```
socket(AF_INET, SOCK_STREAM, 0)
  → allocate TcpSocket (rx_buf 64KB, tx_buf 64KB)
  → add to smoltcp SocketSet
  → insert fd with Socket(handle)
  → return fd

bind(fd, addr:port)
  → store endpoint in fd metadata

listen(fd, backlog)
  → smoltcp socket.listen(port)

accept4(fd, ...) [blocks]
  → poll smoltcp until state == Established
  → if not ready: process.state = Blocked(Accept(handle))
  → scheduler switches away
  → on smoltcp Established event: unblock, return new fd with connected socket

read/recv → smoltcp socket.recv(|buf| ...)
write/send → smoltcp socket.send_slice(...)

epoll_wait
  → iterate registered fds
  → for Socket fds: check smoltcp can_recv/can_send/state
  → return ready events; if none: block until net poll tick
```

### 8. Preemptive Scheduler (`kernel/src/process/scheduler.rs` — extend existing)

IRQ 0 (PIT timer) fires at 100 Hz. The interrupt handler:
1. Saves current process's `CpuContext` from the interrupt stack frame
2. Calls `scheduler.tick()` → returns next `Ready` process (round-robin)
3. Loads new process's `CpuContext` into the interrupt frame
4. Switches CR3 to new process's page table
5. `IRETQ` — resumes the new process transparently

Blocking: syscalls that would block (accept, read on empty socket, futex wait, nanosleep) set `process.state = Blocked(reason)` and call `scheduler.yield_current()`.

Unblocking: network poll tick checks all `Blocked(Accept/Recv)` processes; timer tick unblocks `Blocked(Sleep)` processes.

### 9. OCI Pull Pipeline (wire existing pieces)

```
fn container_start(spec: ContainerSpec) {
    let mut reg = Registry::new("registry-1.docker.io");
    reg.authenticate(&spec.image)?;
    let manifest = reg.fetch_manifest(&spec.image, &spec.tag)?;

    let mut fs = OverlayMount::new();
    for layer in &manifest.layers {
        let data = reg.fetch_layer(&spec.image, &layer.digest)?;
        let tar  = layer::decompress(&data)?;
        fs.apply_tar_layer(&tar)?;
    }

    let elf = ElfLoader::load(&fs, "/usr/sbin/nginx", &["-g", "daemon off;"])?;
    let proc = Process::spawn(elf.entry, elf.stack, fs, spec);
    SCHEDULER.lock().add(proc);
}
```

---

## Implementation Phases

| Phase | New files | Key milestone |
|-------|-----------|---------------|
| 1. Execution foundation | `exec/elf.rs`, `exec/entry.asm`, `exec/syscall.rs`, `memory/user.rs` | Static ELF binary runs in user space |
| 2. File I/O | `exec/fd_table.rs`, `exec/procfs.rs`, `exec/devfs.rs` | Process reads rootfs, /proc, /dev |
| 3. Dynamic linking | extend elf.rs (PT_INTERP), extend syscall.rs | ld-linux loads, glibc initialises |
| 4. Network | `exec/socket.rs`, extend syscall.rs | nginx binds :80, accepts connections |
| 5. Process model | extend scheduler.rs, add fork/clone/futex | nginx master + workers run concurrently |
| 6. OCI pull end-to-end | wire registry + layer + overlayfs + exec | `nginx:latest` pulled and serving HTTP |

---

## Syscall Count by Phase

| Phase | Syscalls added | Running total |
|-------|---------------|---------------|
| 1 | exit, exit_group, write, read, mmap, munmap, brk, arch_prctl, set_tid_address, uname | 10 |
| 2 | openat, close, fstat, stat, lstat, readv, writev, lseek, access, faccessat, getcwd, getdents64, readlinkat, dup, dup2, dup3, fcntl, ioctl, pipe2 | 29 |
| 3 | mprotect, mremap, madvise, getuid, getgid, geteuid, getegid, getpid, gettid, prctl, prlimit64, set_robust_list, setsockopt (for TLS in ld-linux), getrandom | 43 |
| 4 | socket, bind, connect, listen, accept4, getsockname, getpeername, setsockopt, getsockopt, sendto, recvfrom, sendmsg, recvmsg, shutdown, sendfile, epoll_create1, epoll_ctl, epoll_wait, epoll_pwait, poll | 63 |
| 5 | clone, fork, wait4, waitid, futex, rt_sigaction, rt_sigprocmask, rt_sigreturn, sigaltstack, kill, tgkill, nanosleep, clock_gettime, gettimeofday, setitimer, setsid, setpgid, getppid, umask | 82 |
| 6 | eventfd2, timerfd_create/settime/gettime, pipe, socketpair, sendmmsg, recvmmsg, clock_nanosleep, sysinfo, times, chdir, mkdir, unlink, rename, getrlimit | ~100 |
