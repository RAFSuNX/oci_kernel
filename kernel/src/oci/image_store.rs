extern crate alloc;
use alloc::{string::String, vec::Vec};
use alloc::collections::BTreeMap;

use super::manifest::ImageManifest;

/// Metadata about a stored layer (we keep data in memory for now).
pub struct LayerEntry {
    pub digest: String,
    pub size:   usize,
    pub data:   Vec<u8>,    // raw compressed bytes
}

/// Content-addressed store for OCI image layers and manifests.
pub struct ImageStore {
    layers: BTreeMap<String, LayerEntry>,           // digest -> entry
    images: BTreeMap<String, ImageManifest>,         // "name:tag" -> manifest
}

impl ImageStore {
    pub fn new() -> Self {
        Self {
            layers: BTreeMap::new(),
            images: BTreeMap::new(),
        }
    }

    /// For tests — same as new() since we're in-memory.
    pub fn new_test() -> Self { Self::new() }

    /// Check if a layer with the given digest is already stored.
    pub fn has_layer(&self, digest: &str) -> bool {
        self.layers.contains_key(digest)
    }

    /// Store a layer by digest. No-op if already present (content-addressed dedup).
    pub fn store_layer(&mut self, digest: &str, data: &[u8]) -> Result<(), StoreError> {
        if self.has_layer(digest) {
            return Ok(()); // deduplicated
        }
        self.layers.insert(digest.into(), LayerEntry {
            digest: digest.into(),
            size:   data.len(),
            data:   data.to_vec(),
        });
        Ok(())
    }

    /// Retrieve raw layer bytes by digest.
    pub fn get_layer(&self, digest: &str) -> Option<&[u8]> {
        self.layers.get(digest).map(|e| e.data.as_slice())
    }

    /// Number of stored layers (for dedup testing).
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Store an image manifest under "name:tag" key.
    pub fn store_image(&mut self, name: &str, tag: &str, manifest: ImageManifest) {
        let key = alloc::format!("{}:{}", name, tag);
        self.images.insert(key, manifest);
    }

    /// Retrieve an image manifest by name and tag.
    pub fn get_image(&self, name: &str, tag: &str) -> Option<&ImageManifest> {
        let key = alloc::format!("{}:{}", name, tag);
        self.images.get(&key)
    }
}

#[derive(Debug)]
pub enum StoreError {
    OutOfMemory,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve_layer() {
        let mut store = ImageStore::new_test();
        let digest = "sha256:abc123";
        let data = alloc::vec![1u8, 2, 3, 4];
        store.store_layer(digest, &data).unwrap();
        assert!(store.has_layer(digest));
    }

    #[test]
    fn layer_deduplication() {
        let mut store = ImageStore::new_test();
        let digest = "sha256:abc123";
        let data = alloc::vec![1u8, 2, 3];
        store.store_layer(digest, &data).unwrap();
        store.store_layer(digest, &data).unwrap(); // second call — no duplicate
        assert_eq!(store.layer_count(), 1);
    }

    #[test]
    fn retrieve_layer_data() {
        let mut store = ImageStore::new_test();
        let digest = "sha256:deadbeef";
        let data = alloc::vec![0xde, 0xad, 0xbe, 0xef];
        store.store_layer(digest, &data).unwrap();
        assert_eq!(store.get_layer(digest).unwrap(), &[0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn missing_layer_returns_none() {
        let store = ImageStore::new_test();
        assert!(store.get_layer("sha256:missing").is_none());
    }
}
