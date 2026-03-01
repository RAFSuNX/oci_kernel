extern crate alloc;

use super::spec::ContainerSpec;
use crate::isolation::{
    namespace::Namespace,
    cgroup::CgroupHandle,
    seccomp::SeccompFilter,
};
use crate::fs::overlayfs::OverlayMount;

// Monotonic counter used to assign unique container IDs.
// In test mode we use a simple static; in kernel mode we use an AtomicU64.
#[cfg(not(test))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(test))]
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
static NEXT_ID: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// An opaque, globally-unique container identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContainerId(pub u64);

impl ContainerId {
    pub fn new() -> Self {
        ContainerId(next_id())
    }
}

/// OCI container lifecycle state machine.
///
/// ```text
/// Created → Running → Stopped → (removed)
///                  ↘ Stopping
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContainerState {
    /// Container created but init process not yet started.
    Created,
    /// Init process running.
    Running,
    /// Graceful stop in progress (SIGTERM sent, waiting for exit).
    Stopping,
    /// Process has exited.
    Stopped,
}

/// A live container instance.
pub struct Container {
    pub id:        ContainerId,
    pub state:     ContainerState,
    pub spec:      ContainerSpec,
    pub namespace: Namespace,
    pub cgroup:    CgroupHandle,
    pub seccomp:   SeccompFilter,
    pub rootfs:    OverlayMount,
}

impl Container {
    /// Create a container from a spec. Does NOT start the init process.
    pub fn create(spec: ContainerSpec) -> Self {
        let memory_limit = if spec.resources.memory_bytes == 0 {
            256 * 1024 * 1024 // 256 MB default
        } else {
            spec.resources.memory_bytes
        };

        Self {
            id:        ContainerId::new(),
            state:     ContainerState::Created,
            namespace: Namespace::new_isolated(),
            cgroup:    CgroupHandle::new(memory_limit),
            seccomp:   SeccompFilter::default_policy(),
            rootfs:    OverlayMount::new(alloc::vec![]),
            spec,
        }
    }

    /// Transition to Running. Returns Err if already running or stopped.
    pub fn start(&mut self) -> Result<(), &'static str> {
        match self.state {
            ContainerState::Created => {
                self.state = ContainerState::Running;
                Ok(())
            }
            _ => Err("cannot start: not in Created state"),
        }
    }

    /// Begin graceful shutdown.
    pub fn stop(&mut self) -> Result<(), &'static str> {
        match self.state {
            ContainerState::Running => {
                self.state = ContainerState::Stopping;
                Ok(())
            }
            _ => Err("cannot stop: not in Running state"),
        }
    }

    /// Mark the container as fully stopped (called when the init process exits).
    pub fn mark_stopped(&mut self) {
        self.state = ContainerState::Stopped;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::spec::ContainerSpec;

    #[test]
    fn container_starts_in_created_state() {
        let spec = ContainerSpec::test_default();
        let container = Container::create(spec);
        assert_eq!(container.state, ContainerState::Created);
    }

    #[test]
    fn container_id_is_unique() {
        let a = ContainerId::new();
        let b = ContainerId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn container_lifecycle_states() {
        let spec = ContainerSpec::test_default();
        let mut c = Container::create(spec);
        assert_eq!(c.state, ContainerState::Created);
        c.start().unwrap();
        assert_eq!(c.state, ContainerState::Running);
        c.stop().unwrap();
        assert_eq!(c.state, ContainerState::Stopping);
        c.mark_stopped();
        assert_eq!(c.state, ContainerState::Stopped);
    }

    #[test]
    fn cannot_start_already_running_container() {
        let spec = ContainerSpec::test_default();
        let mut c = Container::create(spec);
        c.start().unwrap();
        assert!(c.start().is_err());
    }

    #[test]
    fn cannot_stop_created_container() {
        let spec = ContainerSpec::test_default();
        let c = Container::create(spec);
        // Would need mut, but we can test via a separate mutable binding
        let mut c = c;
        // stop() requires Running state
        assert!(c.stop().is_err());
        // start first, then stop should work
        c.start().unwrap();
        assert!(c.stop().is_ok());
    }
}
