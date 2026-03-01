extern crate alloc;
use alloc::string::String;

/// An opaque PID within a namespace. Always starts at 1 in a fresh namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub u64);

/// PID namespace: each container gets its own PID 1.
pub struct PidNamespace {
    next_pid: u64,
}

impl PidNamespace {
    pub fn new() -> Self {
        Self { next_pid: 1 }
    }

    /// Allocate the next PID in this namespace.
    pub fn allocate(&mut self) -> ProcessId {
        let pid = ProcessId(self.next_pid);
        self.next_pid += 1;
        pid
    }
}

/// UTS namespace: per-container hostname and domain name.
pub struct UtsNamespace {
    pub hostname: String,
    pub domainname: String,
}

impl UtsNamespace {
    pub fn new() -> Self {
        Self {
            hostname: String::from("container"),
            domainname: String::new(),
        }
    }
}

/// User namespace: UID/GID mapping — container root (0) maps to unprivileged host UID.
pub struct UserNamespace {
    /// uid_map[container_uid] = host_uid
    uid_offset: u32,
    gid_offset: u32,
}

impl UserNamespace {
    /// Create a user namespace mapping container UID 0 → host UID `uid_base`.
    pub fn new() -> Self {
        // Default: map container root to a high host UID (no real privilege)
        Self { uid_offset: 100_000, gid_offset: 100_000 }
    }

    pub fn container_to_host_uid(&self, container_uid: u32) -> u32 {
        self.uid_offset + container_uid
    }

    pub fn container_to_host_gid(&self, container_gid: u32) -> u32 {
        self.gid_offset + container_gid
    }
}

/// IPC namespace: isolated POSIX message queues and semaphores.
pub struct IpcNamespace {
    // Placeholder: actual IPC objects added when IPC is implemented.
    pub id: u64,
}

impl IpcNamespace {
    pub fn new() -> Self {
        Self { id: 0 }
    }
}

/// Combined namespace set for one container.
pub struct Namespace {
    pub pid:  PidNamespace,
    pub uts:  UtsNamespace,
    pub user: UserNamespace,
    pub ipc:  IpcNamespace,
}

impl Namespace {
    pub fn new_isolated() -> Self {
        Self {
            pid:  PidNamespace::new(),
            uts:  UtsNamespace::new(),
            user: UserNamespace::new(),
            ipc:  IpcNamespace::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_namespace_starts_at_one() {
        let mut ns = PidNamespace::new();
        let pid = ns.allocate();
        assert_eq!(pid.0, 1); // first process is always PID 1
    }

    #[test]
    fn pid_namespaces_are_isolated() {
        let mut ns_a = PidNamespace::new();
        let mut ns_b = PidNamespace::new();
        let pid_a = ns_a.allocate();
        let pid_b = ns_b.allocate();
        assert_eq!(pid_a.0, pid_b.0); // both PID 1, different namespaces
    }

    #[test]
    fn pid_namespace_increments() {
        let mut ns = PidNamespace::new();
        assert_eq!(ns.allocate().0, 1);
        assert_eq!(ns.allocate().0, 2);
        assert_eq!(ns.allocate().0, 3);
    }

    #[test]
    fn user_namespace_maps_root_to_high_uid() {
        let uns = UserNamespace::new();
        // Container root (uid 0) must NOT map to host root (uid 0)
        assert_ne!(uns.container_to_host_uid(0), 0);
    }

    #[test]
    fn new_isolated_namespace_composes_all_sub_namespaces() {
        let ns = Namespace::new_isolated();
        assert_eq!(ns.uts.hostname, "container");
    }
}
