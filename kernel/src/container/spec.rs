extern crate alloc;
use alloc::{string::String, vec::Vec};

/// How host ports map to container ports.
#[derive(Debug, Clone)]
pub struct PortMapping {
    pub host:      u16,
    pub container: u16,
}

/// Volume accessibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccessMode {
    ReadOnly,
    ReadWrite,
}

/// A host-directory or named-volume mount.
#[derive(Debug, Clone)]
pub struct VolumeMount {
    pub source: String, // host path or volume name
    pub target: String, // path inside container
    pub access: AccessMode,
}

/// Container restart behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

/// Networking mode for the container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetworkMode {
    /// Container gets its own vNIC on the virtual switch (default).
    Bridge,
    /// Container shares the host network stack (no isolation).
    Host,
    /// No network access.
    None,
}

/// Resource limits enforced by the cgroup.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub memory_bytes: usize, // hard limit; 0 = unlimited (256MB default)
    pub pids_max:     usize, // max simultaneous processes
    pub cpu_shares:   u32,   // relative CPU weight (1024 = default)
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 256 * 1024 * 1024, // 256 MB
            pids_max:     100,
            cpu_shares:   1024,
        }
    }
}

/// Full description of how to create and run a container.
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    pub image:     String,
    pub command:   Vec<String>,
    pub env:       Vec<(String, String)>,
    pub ports:     Vec<PortMapping>,
    pub volumes:   Vec<VolumeMount>,
    pub network:   NetworkMode,
    pub resources: ResourceLimits,
    pub restart:   RestartPolicy,
}

impl ContainerSpec {
    pub fn new(image: impl Into<String>, command: Vec<String>) -> Self {
        Self {
            image:     image.into(),
            command,
            env:       Vec::new(),
            ports:     Vec::new(),
            volumes:   Vec::new(),
            network:   NetworkMode::Bridge,
            resources: ResourceLimits::default(),
            restart:   RestartPolicy::Never,
        }
    }

    /// Convenience constructor for unit tests.
    #[cfg(test)]
    pub fn test_default() -> Self {
        Self::new("test-image:latest", alloc::vec![String::from("/bin/sh")])
    }
}
