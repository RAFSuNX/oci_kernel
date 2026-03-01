/// Errors from cgroup enforcement.
#[derive(Debug, PartialEq)]
pub enum CgroupError {
    MemoryLimit,
    PidLimit,
}

/// A cgroup handle tracking resource limits for one container.
///
/// Tracks memory and PID usage. CPU shares are advisory only for now
/// (actual scheduling is handled by the kernel scheduler).
pub struct CgroupHandle {
    memory_limit:  usize, // bytes
    memory_used:   usize, // bytes
    cpu_shares:    u32,   // relative weight (1024 = default)
    pids_max:      usize, // max concurrent processes
    pids_current:  usize,
}

impl CgroupHandle {
    pub fn new(memory_limit: usize) -> Self {
        Self {
            memory_limit,
            memory_used: 0,
            cpu_shares: 1024,
            pids_max: 100,
            pids_current: 0,
        }
    }

    /// Check if allocating `request` bytes would exceed the memory limit.
    /// Does NOT commit the allocation — caller must call `charge_memory` after
    /// a successful check if the allocation proceeds.
    pub fn check_memory(&self, request: usize) -> Result<(), CgroupError> {
        if self.memory_used.saturating_add(request) > self.memory_limit {
            Err(CgroupError::MemoryLimit)
        } else {
            Ok(())
        }
    }

    /// Record that `bytes` were allocated.
    pub fn charge_memory(&mut self, bytes: usize) {
        self.memory_used = self.memory_used.saturating_add(bytes);
    }

    /// Record that `bytes` were freed.
    pub fn release_memory(&mut self, bytes: usize) {
        self.memory_used = self.memory_used.saturating_sub(bytes);
    }

    /// Check if spawning another process is allowed.
    pub fn check_pids(&self) -> Result<(), CgroupError> {
        if self.pids_current >= self.pids_max {
            Err(CgroupError::PidLimit)
        } else {
            Ok(())
        }
    }

    /// Record that a new process started.
    pub fn charge_pid(&mut self) {
        self.pids_current = self.pids_current.saturating_add(1);
    }

    /// Record that a process exited.
    pub fn release_pid(&mut self) {
        self.pids_current = self.pids_current.saturating_sub(1);
    }

    pub fn memory_used(&self) -> usize  { self.memory_used }
    pub fn memory_limit(&self) -> usize { self.memory_limit }
    pub fn pids_current(&self) -> usize { self.pids_current }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgroup_memory_enforced() {
        let cgroup = CgroupHandle::new(1024 * 1024); // 1 MB limit
        assert!(cgroup.check_memory(512 * 1024).is_ok());
        assert!(cgroup.check_memory(2 * 1024 * 1024).is_err()); // over limit
    }

    #[test]
    fn cgroup_memory_charge_release() {
        let mut cgroup = CgroupHandle::new(1024);
        cgroup.charge_memory(512);
        assert_eq!(cgroup.memory_used(), 512);
        assert!(cgroup.check_memory(512).is_ok());
        assert!(cgroup.check_memory(513).is_err()); // 512+513 > 1024
        cgroup.release_memory(256);
        assert_eq!(cgroup.memory_used(), 256);
    }

    #[test]
    fn cgroup_pids_enforced() {
        let mut cgroup = CgroupHandle::new(1024 * 1024);
        // Force pids_max to 1 via charge
        for _ in 0..100 {
            cgroup.charge_pid();
        }
        assert!(cgroup.check_pids().is_err());
        cgroup.release_pid();
        assert!(cgroup.check_pids().is_ok());
    }

    #[test]
    fn saturating_release_does_not_underflow() {
        let mut cgroup = CgroupHandle::new(1024);
        cgroup.release_memory(9999); // nothing to release
        assert_eq!(cgroup.memory_used(), 0);
    }
}
