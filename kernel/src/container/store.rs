extern crate alloc;
use alloc::{collections::BTreeMap, vec::Vec};

use super::runtime::{ContainerId, ContainerState};

/// A record of a container held in the store.
#[derive(Debug)]
pub struct ContainerRecord {
    pub id:    ContainerId,
    pub state: ContainerState,
}

/// Kernel-global registry of all known containers.
pub struct ContainerStore {
    entries: BTreeMap<u64, ContainerRecord>,
}

impl ContainerStore {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    /// Register a new container with an initial state.
    pub fn register(&mut self, id: ContainerId, state: ContainerState) {
        self.entries.insert(id.0, ContainerRecord { id, state });
    }

    pub fn get(&self, id: ContainerId) -> Option<&ContainerRecord> {
        self.entries.get(&id.0)
    }

    pub fn get_mut(&mut self, id: ContainerId) -> Option<&mut ContainerRecord> {
        self.entries.get_mut(&id.0)
    }

    pub fn remove(&mut self, id: ContainerId) -> Option<ContainerRecord> {
        self.entries.remove(&id.0)
    }

    /// Number of containers currently in `Running` state.
    pub fn running_count(&self) -> usize {
        self.entries.values().filter(|r| r.state == ContainerState::Running).count()
    }

    /// All containers, regardless of state.
    pub fn all(&self) -> Vec<&ContainerRecord> {
        self.entries.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_store_tracks_running() {
        let mut store = ContainerStore::new();
        let id = ContainerId::new();
        store.register(id, ContainerState::Running);
        assert_eq!(store.get(id).unwrap().state, ContainerState::Running);
        assert_eq!(store.running_count(), 1);
    }

    #[test]
    fn running_count_only_counts_running() {
        let mut store = ContainerStore::new();
        store.register(ContainerId::new(), ContainerState::Running);
        store.register(ContainerId::new(), ContainerState::Stopped);
        store.register(ContainerId::new(), ContainerState::Created);
        assert_eq!(store.running_count(), 1);
    }

    #[test]
    fn remove_container_from_store() {
        let mut store = ContainerStore::new();
        let id = ContainerId::new();
        store.register(id, ContainerState::Stopped);
        assert!(store.remove(id).is_some());
        assert!(store.get(id).is_none());
    }
}
