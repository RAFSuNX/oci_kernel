pub mod spec;
pub mod runtime;
pub mod store;

extern crate alloc;
use alloc::{string::String, vec::Vec};
use spin::{Lazy, Mutex};
use store::ContainerStore;

/// Kernel-global container registry. Populated at boot from the YAML config
/// and updated live as containers are started/stopped from the shell.
pub static STORE: Lazy<Mutex<ContainerStore>> =
    Lazy::new(|| Mutex::new(ContainerStore::new()));

/// In-memory image registry — tracks (name, tag) pairs that have been pulled.
/// Persists for the lifetime of the kernel session.
#[cfg(not(test))]
pub static IMAGE_STORE: Lazy<Mutex<Vec<(String, String)>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Named volume registry — tracks volume names created via `volume create`.
#[cfg(not(test))]
pub static VOLUME_STORE: Lazy<Mutex<Vec<String>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Active port-forward rules populated by `container run -p host:container`.
///
/// Each rule means:
///   QEMU hostfwd tcp::host_port-:host_port
///     → kernel smoltcp socket  (M2: proxy to container)
///       → container_id process on container_port
///
/// In M2 the networking layer reads this table to create smoltcp TCP sockets.
#[cfg(not(test))]
pub static PORT_FORWARDS: Lazy<Mutex<Vec<spec::ActivePortForward>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
