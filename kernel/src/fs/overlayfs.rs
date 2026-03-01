extern crate alloc;
use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use super::vfs::FsError;

/// An in-memory read-only layer (represents one unpacked OCI image layer).
pub struct MemLayer {
    files: BTreeMap<String, Vec<u8>>,
}

impl MemLayer {
    pub fn new() -> Self {
        Self { files: BTreeMap::new() }
    }

    pub fn insert(&mut self, path: &str, data: &[u8]) {
        self.files.insert(path.into(), data.to_vec());
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

/// The writable upper layer (per-container scratch space).
struct UpperLayer {
    files: BTreeMap<String, Vec<u8>>,
}

impl UpperLayer {
    fn new() -> Self {
        Self { files: BTreeMap::new() }
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).cloned()
    }

    fn write(&mut self, path: &str, data: &[u8]) {
        self.files.insert(path.into(), data.to_vec());
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }
}

/// OverlayMount: N read-only lower layers + one writable upper layer.
///
/// Read order: upper first, then lower layers from top (last) to bottom (first).
/// Write: always goes to upper. Copy-on-write: if the file exists only in lower,
/// it is copied into upper before the write, preserving the lower layer unchanged.
pub struct OverlayMount {
    lower: Vec<Arc<MemLayer>>,
    upper: UpperLayer,
}

impl OverlayMount {
    /// Production constructor: provide lower layers (bottom → top order).
    pub fn new(lower: Vec<Arc<MemLayer>>) -> Self {
        Self { lower, upper: UpperLayer::new() }
    }

    /// Read a file. Upper layer wins; then lower layers searched top → bottom.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, FsError> {
        if let Some(data) = self.upper.read(path) {
            return Ok(data);
        }
        for layer in self.lower.iter().rev() {
            if let Some(data) = layer.read(path) {
                return Ok(data.to_vec());
            }
        }
        Err(FsError::NotFound)
    }

    /// Write a file to the upper layer. CoW: copy from lower first if needed.
    pub fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if !self.upper.exists(path) {
            // Copy-on-write: if the file is in a lower layer, copy it to upper
            // before overwriting so lower stays immutable.
            if let Some(existing) = self.read_lower(path) {
                self.upper.write(path, &existing);
            }
        }
        self.upper.write(path, data);
        Ok(())
    }

    /// Check if a path exists in any layer.
    pub fn exists(&self, path: &str) -> bool {
        if self.upper.exists(path) {
            return true;
        }
        self.lower.iter().any(|l| l.exists(path))
    }

    /// True if the path is present in the upper (writable) layer.
    pub fn upper_exists(&self, path: &str) -> bool {
        self.upper.exists(path)
    }

    /// Read from lower layers only (upper ignored). Used by CoW logic.
    fn read_lower(&self, path: &str) -> Option<Vec<u8>> {
        for layer in self.lower.iter().rev() {
            if let Some(data) = layer.read(path) {
                return Some(data.to_vec());
            }
        }
        None
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

#[cfg(test)]
impl OverlayMount {
    /// Test-only constructor: starts with a single empty lower layer.
    pub fn new_test() -> Self {
        Self::new(alloc::vec![Arc::new(MemLayer::new())])
    }

    /// Write directly into the bottom lower layer (test setup helper).
    pub fn lower_write(&mut self, path: &str, data: &[u8]) {
        Arc::get_mut(&mut self.lower[0])
            .expect("lower layer shared — can't mutate in test")
            .insert(path, data);
    }

    /// Read directly from the bottom lower layer (bypasses upper).
    pub fn lower_read(&self, path: &str) -> &[u8] {
        self.lower[0].read(path).expect("path not in lower layer")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_from_lower_layer() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hosts", b"127.0.0.1 localhost");
        assert_eq!(overlay.read("/etc/hosts").unwrap(), b"127.0.0.1 localhost");
    }

    #[test]
    fn write_goes_to_upper() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hosts", b"original");
        overlay.write("/etc/hosts", b"modified").unwrap();
        // Lower layer must remain unchanged
        assert_eq!(overlay.lower_read("/etc/hosts"), b"original");
        // Container sees the modified version
        assert_eq!(overlay.read("/etc/hosts").unwrap(), b"modified");
    }

    #[test]
    fn upper_takes_precedence() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/file", b"lower");
        overlay.write("/file", b"upper").unwrap();
        assert_eq!(overlay.read("/file").unwrap(), b"upper");
    }

    #[test]
    fn cow_on_first_write() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/file", b"original");
        overlay.write("/file", b"new").unwrap();
        // After a write, the path must exist in the upper layer
        assert!(overlay.upper_exists("/file"));
        // Lower must still hold the original bytes
        assert_eq!(overlay.lower_read("/file"), b"original");
    }

    #[test]
    fn write_new_file_lands_in_upper() {
        let mut overlay = OverlayMount::new_test();
        overlay.write("/new_file", b"hello").unwrap();
        assert!(overlay.upper_exists("/new_file"));
        assert_eq!(overlay.read("/new_file").unwrap(), b"hello");
    }

    #[test]
    fn not_found_returns_error() {
        let overlay = OverlayMount::new_test();
        assert_eq!(overlay.read("/nonexistent"), Err(FsError::NotFound));
    }
}
