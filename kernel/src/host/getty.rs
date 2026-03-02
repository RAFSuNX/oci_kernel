/// Getty: interactive login + shell over COM1 serial.
///
/// Credentials for M1: root / admin (hardcoded).
/// All shell state (cwd, etc.) lives in the Getty struct.
///
/// Hardware-dependent — not compiled for host unit tests.

#[cfg(not(test))]
use alloc::{format, string::String, vec::Vec};

#[cfg(not(test))]
use super::shell::ShellCommand;

#[cfg(not(test))] const ROOT_USER: &str = "root";
#[cfg(not(test))] const ROOT_PASS: &str = "admin";

#[cfg(not(test))]
pub struct Getty {
    cwd: String,
}

#[cfg(not(test))]
impl Getty {
    pub fn new() -> Self {
        Self { cwd: String::from("/home/root") }
    }

    // ── Login loop ────────────────────────────────────────────────────────────

    pub fn run(&mut self) -> ! {
        loop {
            self.println("OCI Kernel 0.1.0");
            self.print("login: ");
            let user = self.read_line();
            self.print("Password: ");
            let pass = self.read_line_silent();

            if user.trim() == ROOT_USER && pass.trim() == ROOT_PASS {
                self.run_shell();
            } else {
                self.println("Login incorrect.\n");
            }
        }
    }

    // ── Shell loop ────────────────────────────────────────────────────────────

    fn run_shell(&mut self) {
        // Show motd
        {
            if let Ok(data) = crate::fs::ROOT_FS.lock().read("/etc/motd") {
                if let Ok(s) = core::str::from_utf8(&data) {
                    self.print(s);
                }
            }
        }

        loop {
            self.print(&format!("{}# ", self.cwd));
            let line = self.read_line();
            if line.trim().is_empty() { continue; }
            match ShellCommand::parse(&line) {
                Ok(cmd)  => self.execute(cmd),
                Err(_)   => self.println(
                    "command not found. Type 'help' for available commands."),
            }
        }
    }

    // ── Command execution ─────────────────────────────────────────────────────

    fn execute(&mut self, cmd: ShellCommand) {
        use crate::container::{
            STORE, IMAGE_STORE, VOLUME_STORE, PORT_FORWARDS,
            runtime::{Container, ContainerState, ContainerId},
            spec::{ContainerSpec, ActivePortForward},
        };

        match cmd {

            // ── ls ────────────────────────────────────────────────────────────
            ShellCommand::Ls { path } => {
                let dir = match &path {
                    Some(p) => self.resolve_path(p),
                    None    => self.cwd.clone(),
                };
                let fs = crate::fs::ROOT_FS.lock();
                let entries = fs.list(&dir);
                if entries.is_empty() {
                    if !fs.is_dir(&dir) {
                        self.println(&format!("ls: {}: No such file or directory", dir));
                    }
                    // (empty directory — no output)
                } else {
                    for e in &entries { self.println(e); }
                }
            }

            // ── cd ────────────────────────────────────────────────────────────
            ShellCommand::Cd { path } => {
                let target = self.resolve_path(&path);
                let exists = crate::fs::ROOT_FS.lock().is_dir(&target);
                if exists {
                    self.cwd = target;
                } else {
                    self.println(&format!("cd: {}: No such directory", path));
                }
            }

            // ── pwd ───────────────────────────────────────────────────────────
            ShellCommand::Pwd => {
                let cwd = self.cwd.clone();
                self.println(&cwd);
            }

            // ── cat ───────────────────────────────────────────────────────────
            ShellCommand::Cat { path } => {
                let p = self.resolve_path(&path);
                match crate::fs::ROOT_FS.lock().read(&p) {
                    Ok(data) => match core::str::from_utf8(&data) {
                        Ok(s)  => self.print(s),
                        Err(_) => self.println(&format!(
                            "<binary file: {} bytes>", data.len())),
                    },
                    Err(_) => self.println(&format!(
                        "cat: {}: No such file or directory", path)),
                }
            }

            // ── echo ──────────────────────────────────────────────────────────
            ShellCommand::Echo { text, file } => {
                if let Some(ref dest) = file {
                    let p = self.resolve_path(dest);
                    let mut line = text.clone();
                    line.push('\n');
                    let _ = crate::fs::ROOT_FS.lock().write(&p, line.as_bytes());
                } else {
                    self.println(&text);
                }
            }

            // ── touch ─────────────────────────────────────────────────────────
            ShellCommand::Touch { path } => {
                let p = self.resolve_path(&path);
                let mut fs = crate::fs::ROOT_FS.lock();
                if !fs.exists(&p) {
                    let _ = fs.write(&p, b"");
                }
            }

            // ── mkdir ─────────────────────────────────────────────────────────
            ShellCommand::Mkdir { path } => {
                let p = self.resolve_path(&path);
                // Create a hidden marker file so the dir is discoverable.
                let keep = format!("{}/.keep", p.trim_end_matches('/'));
                let _ = crate::fs::ROOT_FS.lock().write(&keep, b"");
            }

            // ── rm ────────────────────────────────────────────────────────────
            ShellCommand::Rm { path } => {
                let p = self.resolve_path(&path);
                match crate::fs::ROOT_FS.lock().remove(&p) {
                    Ok(())  => {}
                    Err(crate::fs::vfs::FsError::PermissionDenied) =>
                        self.println(&format!("rm: {}: permission denied (read-only layer)", path)),
                    Err(_) =>
                        self.println(&format!("rm: {}: No such file or directory", path)),
                }
            }

            // ── clear ─────────────────────────────────────────────────────────
            ShellCommand::Clear => {
                self.print("\x1b[2J\x1b[H"); // ANSI: erase screen + cursor home
            }

            // ── uname ─────────────────────────────────────────────────────────
            ShellCommand::Uname { all } => {
                if all {
                    self.println("OCI Kernel 0.1.0 oci-kernel 0.1.0 #1 SMP x86_64");
                } else {
                    self.println("OCI Kernel");
                }
            }

            // ── free ──────────────────────────────────────────────────────────
            ShellCommand::Free => {
                const HEAP_SIZE: usize = 8 * 1024 * 1024;
                self.println("              total");
                self.println(&format!("Heap:      {:>8} KB  (at 0xFFFF_C000_0000_0000)",
                    HEAP_SIZE / 1024));
            }

            // ── lsblk — PCI mass-storage scan ─────────────────────────────────
            ShellCommand::Lsblk => {
                use x86_64::instructions::port::Port;
                self.println("NAME       TYPE      PCI ID");
                self.println("─────────  ────────  ──────────────");

                let mut found = false;
                // Scan bus 0 — sufficient for QEMU and typical single-bus systems.
                for dev in 0u8..32 {
                    for func in 0u8..8 {
                        let addr: u32 = 0x8000_0000
                            | ((dev  as u32) << 11)
                            | ((func as u32) << 8);

                        let vendor_device = unsafe {
                            Port::<u32>::new(0xCF8).write(addr);
                            Port::<u32>::new(0xCFC).read()
                        };
                        if vendor_device == 0xFFFF_FFFF { continue; }

                        let class_info = unsafe {
                            Port::<u32>::new(0xCF8).write(addr | 0x08);
                            Port::<u32>::new(0xCFC).read()
                        };
                        let class    = (class_info >> 24) as u8;
                        let subclass = ((class_info >> 16) & 0xFF) as u8;

                        if class == 0x01 {
                            let kind = match subclass {
                                0x01 => "IDE/ATA",
                                0x06 => "SATA",
                                0x07 => "SAS",
                                0x08 => "NVMe",
                                _    => "storage",
                            };
                            let vendor = (vendor_device & 0xFFFF) as u16;
                            let device = ((vendor_device >> 16) & 0xFFFF) as u16;
                            self.println(&format!(
                                "00:{:02x}.{}  {:<8}  {:04x}:{:04x}",
                                dev, func, kind, vendor, device));
                            found = true;
                        }

                        // If function 0 reports single-function device, skip 1-7.
                        if func == 0 {
                            let hdr_type = unsafe {
                                Port::<u32>::new(0xCF8).write(addr | 0x0C);
                                (Port::<u32>::new(0xCFC).read() >> 16) as u8
                            };
                            if hdr_type & 0x80 == 0 { break; }
                        }
                    }
                }

                if !found {
                    self.println("(no mass-storage controllers detected)");
                }
            }

            // ── df ────────────────────────────────────────────────────────────
            ShellCommand::Df => {
                self.println("Filesystem   Type       Mounted on");
                self.println("─────────────────────────────────────────────");
                self.println("rootfs       overlayfs  /          (in-memory)");
                self.println("tmpfs        overlayfs  /tmp       (in-memory)");

                // Count files visible in the root filesystem.
                let fs = crate::fs::ROOT_FS.lock();
                let top = fs.list("/");
                self.println(&format!(
                    "\n{} top-level directories/files in root overlay",
                    top.len()));
            }

            // ── ps ────────────────────────────────────────────────────────────
            ShellCommand::Ps => {
                self.println("PID   IMAGE                STATUS");
                self.println("────  ───────────────────  ─────────");
                let store = STORE.lock();
                let all = store.all();
                if all.is_empty() {
                    self.println("(no processes)");
                } else {
                    for (i, r) in all.iter().enumerate() {
                        let status = match r.state {
                            ContainerState::Running  => "running",
                            ContainerState::Created  => "created",
                            ContainerState::Stopping => "stopping",
                            ContainerState::Stopped  => "stopped",
                        };
                        self.println(&format!(
                            "{:<4}  {:<19}  {}", i + 1, r.image, status));
                    }
                }
            }

            // ── container list ────────────────────────────────────────────────
            ShellCommand::ContainerList => {
                let store = STORE.lock();
                let all = store.all();
                if all.is_empty() {
                    self.println("No containers.");
                } else {
                    self.println("ID                IMAGE                STATUS");
                    self.println("────────────────  ───────────────────  ─────────");
                    for r in all {
                        let status = match r.state {
                            ContainerState::Running  => "running",
                            ContainerState::Created  => "created",
                            ContainerState::Stopping => "stopping",
                            ContainerState::Stopped  => "stopped",
                        };
                        self.println(&format!(
                            "{:<16}  {:<19}  {}", r.id.0, r.image, status));
                    }
                }
            }

            // ── container run ─────────────────────────────────────────────────
            ShellCommand::ContainerRun { image, ports, volumes } => {
                // Clone port list before moving into spec so we can use it below.
                let port_list = ports.clone();
                let mut spec = ContainerSpec::new(image.clone(), alloc::vec![]);
                spec.ports   = ports;
                spec.volumes = volumes;
                let mut c = Container::create(spec);
                match c.start() {
                    Ok(()) => {
                        let id = c.id.0;
                        STORE.lock().register(c.id, image.clone(), ContainerState::Running);
                        self.println(&format!("Container {} started  image={}", id, image));

                        // Register port-forward rules and print access hint.
                        if !port_list.is_empty() {
                            let mut qemu_fwds = String::new();
                            let mut pf_store = PORT_FORWARDS.lock();
                            for pm in &port_list {
                                pf_store.push(ActivePortForward {
                                    container_id:   id,
                                    host_port:      pm.host,
                                    container_port: pm.container,
                                });
                                self.println(&format!(
                                    "  Port mapping: host:{} → container:{}",
                                    pm.host, pm.container));
                                qemu_fwds.push_str(&format!(
                                    ",hostfwd=tcp::{}-:{}", pm.host, pm.container));
                            }
                            drop(pf_store);
                            self.println("  Access:");
                            self.println(&format!(
                                "    QEMU only  : make qemu HOSTFWD=\"{}\"",
                                qemu_fwds));
                            self.println(&format!(
                                "                 curl http://localhost:{}",
                                port_list[0].host));
                            self.println(&format!(
                                "    Real HW    : connect to <machine-ip>:{}",
                                port_list[0].host));
                            self.println("                 (no forwarding needed — direct NIC)");
                        }
                        self.println("  (M2: actual process needs image pull + overlayfs)");
                    }
                    Err(e) => self.println(&format!("Error: {}", e)),
                }
            }

            // ── container stop ────────────────────────────────────────────────
            ShellCommand::ContainerStop { id } => {
                match id.trim().parse::<u64>() {
                    Err(_) => self.println("Error: container ID must be a number."),
                    Ok(n)  => {
                        let mut store = STORE.lock();
                        match store.get_mut(ContainerId(n)) {
                            None    => self.println("Error: container not found."),
                            Some(r) => {
                                r.state = ContainerState::Stopped;
                                self.println(&format!("Container {} stopped.", n));
                            }
                        }
                    }
                }
            }

            // ── container inspect ─────────────────────────────────────────────
            ShellCommand::ContainerInspect { id } => {
                match id.trim().parse::<u64>() {
                    Err(_) => self.println("Error: container ID must be a number."),
                    Ok(n)  => {
                        let store = STORE.lock();
                        match store.get(ContainerId(n)) {
                            None    => self.println("Error: container not found."),
                            Some(r) => {
                                let status = match r.state {
                                    ContainerState::Running  => "running",
                                    ContainerState::Created  => "created",
                                    ContainerState::Stopping => "stopping",
                                    ContainerState::Stopped  => "stopped",
                                };
                                self.println(&format!("ID:     {}", r.id.0));
                                self.println(&format!("Image:  {}", r.image));
                                self.println(&format!("Status: {}", status));
                            }
                        }
                    }
                }
            }

            // ── container logs ────────────────────────────────────────────────
            ShellCommand::ContainerLogs { id, .. } => {
                match id.trim().parse::<u64>() {
                    Err(_) => self.println("Error: container ID must be a number."),
                    Ok(n)  => {
                        let store = STORE.lock();
                        match store.get(ContainerId(n)) {
                            None    => self.println("Error: container not found."),
                            Some(r) => {
                                self.println(&format!(
                                    "[boot ] Container {} ({}) starting...",
                                    r.id.0, r.image));
                                self.println(&format!(
                                    "[boot ] Container {} started  pid=1",
                                    r.id.0));
                                if r.state == ContainerState::Stopped {
                                    self.println(&format!(
                                        "[event] Container {} stopped.", r.id.0));
                                }
                                self.println(
                                    "(M2: live process stdout will stream here)");
                            }
                        }
                    }
                }
            }

            // ── image list ────────────────────────────────────────────────────
            ShellCommand::ImageList => {
                let store = IMAGE_STORE.lock();
                if store.is_empty() {
                    self.println("No images. Use 'image pull <name:tag>' to add one.");
                } else {
                    self.println("REPOSITORY           TAG");
                    self.println("───────────────────  ──────────");
                    for (name, tag) in store.iter() {
                        self.println(&format!("{:<19}  {}", name, tag));
                    }
                }
            }

            // ── image pull ────────────────────────────────────────────────────
            ShellCommand::ImagePull { name, tag } => {
                self.println(&format!("Pulling {}:{}...", name, tag));
                self.println("  (M2: HTTPS + OCI registry pull not yet wired)");
                // Cache in image store so `image list` reflects it.
                let mut store = IMAGE_STORE.lock();
                let already = store.iter().any(|(n, t)| n == &name && t == &tag);
                if !already {
                    store.push((name.clone(), tag.clone()));
                    self.println(&format!("Cached {}:{} locally.", name, tag));
                } else {
                    self.println(&format!("{}:{} already present.", name, tag));
                }
            }

            // ── image rm ──────────────────────────────────────────────────────
            ShellCommand::ImageRemove { name, tag } => {
                let mut store = IMAGE_STORE.lock();
                let before = store.len();
                store.retain(|(n, t)| !(n == &name && t == &tag));
                if store.len() < before {
                    self.println(&format!("Removed {}:{}.", name, tag));
                } else {
                    self.println(&format!("{}:{} not found.", name, tag));
                }
            }

            // ── volume create ─────────────────────────────────────────────────
            ShellCommand::VolumeCreate { name } => {
                let mut store = VOLUME_STORE.lock();
                if store.iter().any(|n| n == &name) {
                    self.println(&format!("Volume '{}' already exists.", name));
                } else {
                    store.push(name.clone());
                    self.println(&format!("Volume '{}' created.", name));
                }
            }

            // ── volume rm ─────────────────────────────────────────────────────
            ShellCommand::VolumeRemove { name } => {
                let mut store = VOLUME_STORE.lock();
                let before = store.len();
                store.retain(|n| n != &name);
                if store.len() < before {
                    self.println(&format!("Volume '{}' removed.", name));
                } else {
                    self.println(&format!("Volume '{}' not found.", name));
                }
            }

            // ── help ──────────────────────────────────────────────────────────
            ShellCommand::Help => {
                self.println("Filesystem:");
                self.println("  ls [path]     cd [path]     pwd");
                self.println("  cat <file>    echo <text> [> file]");
                self.println("  touch <file>  mkdir <dir>   rm <file>");
                self.println("System:");
                self.println("  uname [-a]    free          df");
                self.println("  lsblk         ps            clear");
                self.println("Containers:");
                self.println("  container run <image> [-p host:ctr] [-v src:dst[:ro]]");
                self.println("  container list | ls | ps");
                self.println("  container stop <id> | inspect <id> | logs <id>");
                self.println("  container pull <name:tag>");
                self.println("Images:");
                self.println("  image pull <name:tag> | list | rm <name:tag>");
                self.println("Volumes:");
                self.println("  volume create <name> | rm <name>");
                self.println("Kernel:");
                self.println("  kernel info");
            }

            // ── kernel info ───────────────────────────────────────────────────
            ShellCommand::KernelInfo => {
                self.println("OCI Kernel 0.1.0  arch=x86_64  build=no_std");
                self.println("Heap: 8MB at 0xFFFF_C000_0000_0000");
                self.println("Net:  smoltcp 0.11 / virtio-net / 10.0.2.15/24");
                self.println("      QEMU SLIRP NAT — gateway 10.0.2.2");

                // Show active port-forward rules.
                let pf = PORT_FORWARDS.lock();
                if pf.is_empty() {
                    self.println("Ports: (none — use 'container run -p host:ctr')");
                } else {
                    self.println("Ports:");
                    for r in pf.iter() {
                        self.println(&format!(
                            "  ctr {} : host:{} → container:{}",
                            r.container_id, r.host_port, r.container_port));
                    }
                    // Build the QEMU HOSTFWD string (only needed when running under QEMU;
                    // on real hardware ports are directly accessible via the NIC IP).
                    let fwds: alloc::string::String = pf.iter()
                        .map(|r| format!(",hostfwd=tcp::{}-:{}", r.host_port, r.container_port))
                        .collect::<alloc::vec::Vec<_>>()
                        .join("");
                    drop(pf);
                    self.println(&format!("  QEMU: make qemu HOSTFWD=\"{}\"", fwds));
                    self.println("  Real HW: access via <machine-ip>:<host-port> directly");
                }
            }
        }
    }

    // ── Path helpers ──────────────────────────────────────────────────────────

    /// Resolve a potentially relative path against cwd.
    /// Collapses `.` and `..` components.
    fn resolve_path(&self, path: &str) -> String {
        let raw = if path.starts_with('/') {
            String::from(path)
        } else {
            format!("{}/{}", self.cwd, path)
        };

        let mut parts: Vec<&str> = Vec::new();
        for seg in raw.split('/') {
            match seg {
                "" | "." => {}
                ".."     => { parts.pop(); }
                s        => parts.push(s),
            }
        }

        if parts.is_empty() {
            String::from("/")
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    // ── I/O helpers ───────────────────────────────────────────────────────────

    fn print(&mut self, s: &str) {
        use crate::serial_print;
        serial_print!("{}", s);
    }

    fn println(&mut self, s: &str) {
        use crate::serial_println;
        serial_println!("{}", s);
    }

    /// Read a line, echoing each character.
    fn read_line(&mut self) -> String {
        use crate::serial_print;
        let mut buf = Vec::new();
        loop {
            match read_serial_byte() {
                b'\r' | b'\n' => { serial_print!("\n"); break; }
                0x7f | 0x08  => {
                    if buf.pop().is_some() { serial_print!("\x08 \x08"); }
                }
                c if c >= 0x20 => {
                    buf.push(c);
                    serial_print!("{}", c as char);
                }
                _ => {}
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Read a line without echoing (password input).
    fn read_line_silent(&mut self) -> String {
        use crate::serial_print;
        let mut buf = Vec::new();
        loop {
            match read_serial_byte() {
                b'\r' | b'\n' => { serial_print!("\n"); break; }
                0x7f | 0x08  => { buf.pop(); }
                c if c >= 0x20 => { buf.push(c); }
                _ => {}
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

/// Block until a byte arrives on COM1 (0x3F8).
///
/// While waiting for a keystroke, `net::serve_http_once()` is called so the
/// kernel can respond to HTTP requests without a separate scheduler thread.
#[cfg(not(test))]
fn read_serial_byte() -> u8 {
    use x86_64::instructions::port::Port;
    let mut lsr:  Port<u8> = Port::new(0x3FD); // line status register
    let mut data: Port<u8> = Port::new(0x3F8); // data register
    loop {
        if unsafe { lsr.read() } & 0x01 != 0 {
            return unsafe { data.read() };
        }
        // Poll the smoltcp network stack while idle — serves HTTP concurrently.
        crate::net::serve_http_once();
        core::hint::spin_loop();
    }
}
