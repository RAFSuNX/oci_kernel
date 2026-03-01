# OCI Kernel — Design Document
**Date:** 2026-03-01
**Architecture:** x86_64 monolithic kernel, written in Rust
**Vision:** A kernel where the OCI container runtime is a first-class primitive — no runc, no containerd daemon, no Linux compatibility layer. The kernel IS the container runtime.
**Milestone 1:** Boot, pull nginx:latest from Docker Hub over HTTPS, run it isolated, serve HTTP on port 80.

---

## 1. Goals & Constraints

- **Practical container host** — not a toy, designed to run real OCI workloads
- **x86_64** target architecture
- **Monolithic kernel** — all core services in kernel space
- **Native OCI runtime** — kernel natively speaks OCI Distribution Spec and manages container lifecycle
- **No Linux compatibility** — custom syscall interface, POSIX layer is a future goal
- **Proper isolation** — 6 namespaces + cgroup equivalent + seccomp equivalent, enforced at kernel level
- **Operator access** — serial console, VGA console, SSH, all exposing the same shell

---

## 2. Project Structure

```
oci-kernel/
├── kernel/
│   ├── src/
│   │   ├── main.rs               # Kernel entry point (kernel_main)
│   │   ├── memory/               # Physical + virtual memory
│   │   │   ├── buddy.rs          # Buddy allocator (large allocations)
│   │   │   ├── slab.rs           # Slab allocator (small kernel objects)
│   │   │   └── vm.rs             # Virtual memory, page tables
│   │   ├── interrupts/           # IDT, exceptions, IRQs
│   │   ├── drivers/
│   │   │   ├── virtio_net.rs     # Network card (QEMU/cloud)
│   │   │   ├── virtio_blk.rs     # Block storage
│   │   │   ├── serial.rs         # COM1 serial (debug + console)
│   │   │   ├── vga.rs            # VGA text mode (physical console)
│   │   │   ├── keyboard.rs       # PS/2 keyboard
│   │   │   └── pit.rs            # PIT timer (scheduler ticks)
│   │   ├── net/
│   │   │   ├── stack.rs          # smoltcp TCP/IP integration
│   │   │   ├── tls.rs            # rustls TLS 1.3
│   │   │   ├── http.rs           # HTTP/HTTPS client
│   │   │   └── vswitch.rs        # Virtual switch for container networking
│   │   ├── oci/
│   │   │   ├── registry.rs       # OCI Distribution Spec client
│   │   │   ├── manifest.rs       # Image manifest parser
│   │   │   ├── layer.rs          # Layer pull + decompress + verify
│   │   │   └── image_store.rs    # Local image cache
│   │   ├── fs/
│   │   │   ├── overlayfs.rs      # Union mount for OCI layers (CoW)
│   │   │   ├── tmpfs.rs          # In-memory scratch filesystem
│   │   │   ├── ext2.rs           # Simple disk filesystem for store
│   │   │   └── vfs.rs            # Virtual filesystem interface
│   │   ├── isolation/
│   │   │   ├── namespace.rs      # PID, Mount, Net, UTS, User, IPC
│   │   │   ├── cgroup.rs         # CPU, memory, pids limits
│   │   │   └── seccomp.rs        # Syscall allowlist per container
│   │   ├── container/
│   │   │   ├── runtime.rs        # Full container lifecycle
│   │   │   ├── spec.rs           # ContainerSpec, config.yaml parser
│   │   │   └── store.rs          # Running container registry
│   │   ├── host/
│   │   │   ├── getty.rs          # Login prompt (serial + VGA)
│   │   │   ├── sshd.rs           # SSH server (key-based auth)
│   │   │   └── shell.rs          # Container management shell
│   │   └── syscall/              # Custom kernel syscall interface
│   └── Cargo.toml
├── docs/
│   └── plans/
└── Makefile                      # build, qemu, debug, iso targets
```

### Key Crates

| Crate | Purpose |
|---|---|
| `bootloader` | BIOS/UEFI boot, memory map handoff |
| `x86_64` | Safe CPU/MMU primitives |
| `smoltcp` | No-std TCP/IP stack |
| `rustls` | No-std TLS 1.3 |
| `flate2` / `zstd` | OCI layer decompression |
| `serde_json` | OCI manifest + config parsing |
| `spin` | Kernel spinlocks |
| `linked_list_allocator` | Kernel heap (`Box`, `Vec`, `String`) |
| `pic8259` | Programmable Interrupt Controller |
| `uart_16550` | Serial port driver |

---

## 3. Boot Sequence

```
BIOS/UEFI
  → bootloader crate (long mode, initial page tables)
  → kernel_main(boot_info: &BootInfo)
      → init GDT + IDT
      → init physical memory (buddy + slab allocators)
      → init virtual memory + kernel heap
      → init drivers (serial, VGA, keyboard, PIT, virtio-net, virtio-blk)
      → init network stack (smoltcp + TLS)
      → init disk filesystem + OCI image store
      → init container runtime
      → spawn host processes:
            getty  (serial console)
            getty  (VGA console)
            sshd   (port 22)
      → read /kernel/config.yaml
      → pull + start declared containers
      → enter scheduler loop
```

---

## 4. Memory Layout

```
0x0000_0000_0000_0000  →  container userspace (ring 3, per-container page tables)
0xFFFF_8000_0000_0000  →  physical memory map (identity mapped)
0xFFFF_9000_0000_0000  →  OCI layer cache region (large, growable)
0xFFFF_A000_0000_0000  →  per-container memory regions
0xFFFF_C000_0000_0000  →  kernel heap
0xFFFF_FFFF_8000_0000  →  kernel code/data (.text, .rodata, .bss)
```

### Two-Tier Allocator

```
┌─────────────────────────────────────────┐
│  Slab allocator  (small fixed objects)  │  ← namespace descriptors,
│  4KB–64KB slabs                         │     cgroup handles, file handles
├─────────────────────────────────────────┤
│  Buddy allocator (large variable)       │  ← OCI image layers,
│  4KB → 2MB pages                        │     container memory regions
└─────────────────────────────────────────┘
```

### Per-Container Memory Isolation

Each container gets:
- Its own Level-4 page table (`Cr3`) — completely separate address space
- Memory limit enforced at allocator level
- Guard pages between container regions

```
Container A:              Container B:
┌──────────────┐          ┌──────────────┐
│ overlayfs    │          │ overlayfs    │
│ stack + heap │          │ stack + heap │
│ [GUARD PAGE] │          │ [GUARD PAGE] │
└──────────────┘          └──────────────┘
     ↑ kernel mapped in upper half (shared, read-only from ring 3)
```

---

## 5. Interrupts & Drivers

### IDT Layout

```
0–31    CPU exceptions (Page Fault → kill container | GP Fault | Double Fault → panic)
32–47   Hardware IRQs via PIC8259
  IRQ0  PIT Timer     → scheduler tick (100Hz)
  IRQ1  PS/2 Keyboard → host console input
  IRQ4  Serial COM1   → console + debug
0x80    Syscall vector
```

### Console Drivers (Host Access)

| Driver | Interface | Purpose |
|---|---|---|
| Serial UART | COM1 | Boot debug + serial console |
| VGA text mode | `0xb8000` | Physical monitor output |
| PS/2 Keyboard | IRQ1 | Physical keyboard input |
| PIT Timer | IRQ0 | Scheduler ticks |
| virtio-net | PCI | Network (registry pull + container net) |
| virtio-blk | PCI | Block storage (`/kernel/store/`) |

---

## 6. Container Isolation Primitives

Every container gets **6 isolation boundaries** enforced at kernel level:

| Namespace | Isolates | Container sees |
|---|---|---|
| **PID** | Process IDs | Own PID tree, PID 1 = container init |
| **Mount** | Filesystem view | Own root from overlayfs |
| **Network** | Network stack | Own virtual NIC, IP, routing table |
| **UTS** | Hostname | Own hostname + domain |
| **User** | UID/GID mapping | Root inside = unprivileged outside |
| **IPC** | Shared memory, semaphores | Cannot see other containers |

### Kernel Structs

```rust
struct Namespace {
    pid:   PidNamespace,
    mount: MountNamespace,
    net:   NetNamespace,
    uts:   UtsNamespace,
    user:  UserNamespace,
    ipc:   IpcNamespace,
}

struct CgroupHandle {
    memory_limit: usize,      // max bytes, enforced at allocator
    cpu_shares:   u32,        // scheduler weight
    cpu_quota:    Duration,   // max CPU per period
    pids_max:     usize,      // max processes inside container
    io_weight:    u32,        // block I/O priority
}

struct Container {
    id:        ContainerId,
    namespace: Arc<Namespace>,
    cgroup:    CgroupHandle,
    seccomp:   SeccompFilter,
    state:     ContainerState,   // Created, Running, Paused, Stopped
    rootfs:    OverlayMount,
    net_if:    VirtualNic,
    logs:      LogStream,
}
```

### Seccomp — Default Allowlist

```rust
const ALLOWED: &[Syscall] = &[
    Read, Write, Open, Close, Stat, Mmap,
    Spawn, Exit, Wait, Socket, Connect, Bind, Send, Recv,
];

const BLOCKED: &[Syscall] = &[
    LoadKernelModule,
    RawNetworkAccess,
    MountFilesystem,
    ModifyOtherNamespace,
];
```

---

## 7. Storage Model

### Disk Layout — `/kernel/store/`

```
/kernel/store/
  ├── layers/                        ← OCI image layers (SHA256 content-addressed)
  │     └── sha256:<digest>/         ← shared across all containers using this layer
  ├── images/                        ← image manifests + metadata
  │     └── nginx:latest.json
  ├── containers/
  │     └── <container-id>/
  │           ├── upper/             ← writable layer  (DELETED on container stop)
  │           ├── work/              ← overlayfs work  (DELETED on container stop)
  │           └── logs/              ← stdout/stderr   (PERSISTS after stop)
  └── volumes/                       ← named volumes   (PERSIST always)
        └── <volume-name>/
```

### Storage Rules

| Type | Location | Lifecycle | Writable |
|---|---|---|---|
| OCI layers | `/kernel/store/layers/` | Until image removed | Never |
| Container upper | `/kernel/store/containers/<id>/upper/` | Deleted on stop | Yes |
| Logs | `/kernel/store/containers/<id>/logs/` | Persists after stop | Kernel only |
| Named volume | `/kernel/store/volumes/<name>/` | Persists always | Yes |
| Host path mount | User-declared path | User controlled | Read-only default |

### Security Rules

- Host filesystem is **never** visible to containers by default
- All mounts must be **explicitly declared** — no accidents
- Host path mounts are **read-only** unless `access: readwrite` is declared
- Container upper layer is **ephemeral** — nothing persists without a volume

### Overlay CoW

```rust
impl VfsView {
    fn lookup(&self, path: &Path) -> VfsNode {
        if let Some(node) = self.upper.lookup(path) { return node; }
        for layer in self.lower.iter().rev() {
            if let Some(node) = layer.lookup(path) { return node; }
        }
        VfsNode::NotFound
    }

    fn write(&mut self, path: &Path, data: &[u8]) {
        if !self.upper.exists(path) {
            // CoW: copy from lower layer before first write
            let content = self.lookup(path).read();
            self.upper.create(path, content);
        }
        self.upper.write(path, data);
    }
}
```

---

## 8. Network Stack

### Architecture

```
Physical NIC (virtio-net)
        ↓
   smoltcp (kernel TCP/IP)
        ↓
   ┌──────────────────────────────────┐
   │  kernel network layer            │
   │  ├── registry client (HTTPS)     │
   │  └── vswitch (container routing) │
   └──────────────────────────────────┘
              ↓
   container-A eth0 10.0.0.2
   container-B eth0 10.0.0.3
   container-C eth0 10.0.0.4
```

### Registry Pull (HTTPS)

```
DNS lookup → TCP connect → TLS 1.3 (rustls)
  → GET /v2/<image>/manifests/<tag>
  → parse manifest
  → for each layer:
      HEAD /v2/<image>/blobs/<digest>  ← skip if already local
      GET  /v2/<image>/blobs/<digest>  ← stream + verify SHA256
      decompress (gzip/zstd)
      write to /kernel/store/layers/<digest>/
```

Supported auth: `Anonymous`, `Basic`, `Bearer` (Docker Hub token flow).

### Container Networking — Traffic Rules

```
container → internet:      ALLOWED (NAT via physical NIC)
container → container:     BLOCKED by default
                           ALLOWED if network: shared declared
container → host:          BLOCKED always
host      → container:     port mapping only (explicit -p declaration)
```

IP pool: `10.0.0.0/16`, assigned on start, released on stop.

---

## 9. Container Runtime Lifecycle

```
container run nginx:latest -p 80:80

  1. resolve_image()        → check local store, pull if missing
  2. OverlayMount::new()    → stack read-only layers + upper/
  3. Namespace::new()       → 6 fresh isolated namespaces
  4. CgroupHandle::from()   → apply resource limits
  5. SeccompFilter::apply() → install syscall allowlist
  6. NetNamespace::new()    → assign IP, setup veth pair, apply port map
  7. bind declared volumes  → on top of overlayfs
  8. spawn PID 1            → drop to ring 3, entrypoint executes

container stops
  → SIGTERM to PID 1 → wait grace period → SIGKILL
  → delete upper/, work/
  → keep logs/
  → release IP back to pool
  → named volumes untouched
```

### Restart Policy

| Policy | Behavior |
|---|---|
| `never` | Clean up on exit, done |
| `on-failure` | Restart if exit code != 0, fresh upper/ |
| `always` | Always restart, fresh upper/ |

---

## 10. Operator Interface

### Boot Config — `/kernel/config.yaml`

```yaml
containers:
  - image: nginx:latest
    ports:
      - host: 80
        container: 80
    volumes:
      - source: nginx-data
        target: /var/www/html
        access: readwrite
    restart: always
    resources:
      memory: 512mb
      cpu_shares: 1024
      pids_max: 100

  - image: redis:7
    network: isolated
    restart: on-failure
```

### Shell — Container Management Commands

```bash
container run <image> [options]    # pull if needed + run
container list                     # show running containers
container stop <id>                # graceful stop
container kill <id>                # immediate stop
container logs <id> [--follow]     # stream logs
container inspect <id>             # full state
container rm <id>                  # remove stopped container

image pull <name:tag>              # pull from registry
image list                         # local images
image rm <name:tag>                # remove image

volume create <name>               # create named volume
volume list
volume rm <name>

kernel info                        # version, uptime, resource usage
```

### Access Channels

| Channel | How | When |
|---|---|---|
| Serial console | COM1 → getty → shell | Always (boot, recovery) |
| VGA console | Monitor + PS/2 keyboard → getty → shell | Physical access |
| SSH | Port 22, key-based auth → shell | Remote access |

---

## 11. Custom Syscall Table

### Host-Level (Operator)

| # | Name | Purpose |
|---|---|---|
| 0 | `container_run` | Create + start container |
| 1 | `container_stop` | Graceful stop |
| 2 | `container_list` | List running containers |
| 3 | `container_logs` | Stream stdout/stderr |
| 4 | `container_inspect` | Full container state |
| 5 | `image_pull` | Pull OCI image |
| 6 | `image_list` | List local images |
| 7 | `image_remove` | Delete image |
| 8 | `volume_create` | Create named volume |
| 9 | `volume_remove` | Remove named volume |
| 10 | `kernel_info` | Version, uptime, stats |

### Container-Level (Seccomp Allowlist)

```
read, write, open, close, stat, mmap,
spawn, exit, wait, socket, connect, bind, send, recv
```

---

## 12. Milestone 1 — Definition of Done

**Scenario:** Boot kernel in QEMU, pull nginx:latest from Docker Hub, serve HTTP.

```
$ make qemu
Booting OCI Kernel 0.1.0...
Serial console ready.

login: root
Password:
$ container run nginx:latest -p 80:80
Pulling nginx:latest from docker.io...
  [sha256:a1b2] 45MB ████████████ verified
  [sha256:d4e5] 12MB ████████████ verified
Container ctr-a1b2 running (10.0.0.2 → host:80)

$ curl http://localhost:80
<!DOCTYPE html><html>... nginx default page ...
```

### Checklist

- [ ] Kernel boots in QEMU without panic
- [ ] Serial console shows boot messages
- [ ] Getty spawns on serial, login works
- [ ] VGA console shows boot messages
- [ ] SSH server accepts key-based login
- [ ] virtio-net driver works, kernel has internet access
- [ ] HTTPS registry pull works against Docker Hub
- [ ] Layer SHA256 verification passes
- [ ] Layers stored correctly in `/kernel/store/layers/`
- [ ] Overlayfs mounts correctly (read-only layers + upper/)
- [ ] All 6 namespaces enforced
- [ ] Container runs nginx as PID 1 in ring 3
- [ ] Port 80 mapping works, `curl localhost:80` succeeds
- [ ] stdout/stderr captured to `logs/`
- [ ] Container stop cleans up `upper/`, keeps `logs/`
- [ ] Named volume persists across container restart
- [ ] Host filesystem not visible inside container
