/// The set of syscalls the kernel recognises for seccomp filtering.
///
/// This is a kernel-defined ABI — not Linux's ABI. The kernel will reject
/// any syscall number not listed here before dispatching to the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Syscall {
    // Safe I/O
    Read,
    Write,
    Open,
    Close,
    Stat,
    Seek,
    // Memory
    Mmap,
    Munmap,
    // Process lifecycle
    Spawn,
    Exit,
    Wait,
    // Networking
    Socket,
    Connect,
    Bind,
    Listen,
    Accept,
    Send,
    Recv,
    // Dangerous — never allowed in default policy
    LoadKernelModule,
    ModifyOtherNamespace,
    RawDiskAccess,
    KillAny,
}

/// A seccomp-style allow-list filter.
///
/// Only syscalls in the `allowed` slice may be invoked. Any call to
/// `allow()` for a syscall not in the list returns `false` and the kernel
/// should kill the container process with SIGSYS.
pub struct SeccompFilter {
    allowed: &'static [Syscall],
}

impl SeccompFilter {
    /// Default container policy: permits standard I/O, networking, and
    /// process lifecycle. Explicitly blocks kernel module loading and
    /// cross-namespace operations.
    pub fn default_policy() -> Self {
        Self {
            allowed: &[
                Syscall::Read,
                Syscall::Write,
                Syscall::Open,
                Syscall::Close,
                Syscall::Stat,
                Syscall::Seek,
                Syscall::Mmap,
                Syscall::Munmap,
                Syscall::Spawn,
                Syscall::Exit,
                Syscall::Wait,
                Syscall::Socket,
                Syscall::Connect,
                Syscall::Bind,
                Syscall::Listen,
                Syscall::Accept,
                Syscall::Send,
                Syscall::Recv,
            ],
        }
    }

    /// Returns `true` if the syscall is permitted by this filter.
    pub fn allow(&self, syscall: Syscall) -> bool {
        self.allowed.contains(&syscall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seccomp_blocks_dangerous_syscalls() {
        let filter = SeccompFilter::default_policy();
        assert!(filter.allow(Syscall::Read));
        assert!(filter.allow(Syscall::Write));
        assert!(!filter.allow(Syscall::LoadKernelModule));
        assert!(!filter.allow(Syscall::ModifyOtherNamespace));
    }

    #[test]
    fn seccomp_allows_networking() {
        let filter = SeccompFilter::default_policy();
        assert!(filter.allow(Syscall::Socket));
        assert!(filter.allow(Syscall::Connect));
        assert!(filter.allow(Syscall::Send));
        assert!(filter.allow(Syscall::Recv));
    }

    #[test]
    fn seccomp_blocks_raw_disk() {
        let filter = SeccompFilter::default_policy();
        assert!(!filter.allow(Syscall::RawDiskAccess));
        assert!(!filter.allow(Syscall::KillAny));
    }
}
