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

    /// Pre-populated kernel root filesystem for the getty shell.
    pub fn new_kernel_root() -> Self {
        let mut layer = MemLayer::new();

        // /etc
        layer.insert("/etc/hostname",   b"oci-kernel");
        layer.insert("/etc/os-release",
            b"NAME=OCI Kernel\nVERSION=0.1.0\nID=oci-kernel\n\
              PRETTY_NAME=OCI Kernel 0.1.0\n");
        layer.insert("/etc/motd",
            b"Welcome to OCI Kernel 0.1.0\n\
              The kernel IS the container runtime.\n\
              Type 'help' for available commands.\n");

        // /proc
        layer.insert("/proc/version",   b"OCI Kernel 0.1.0 (x86_64 no_std Rust) #1");
        layer.insert("/proc/cpuinfo",   b"processor\t: 0\nmodel name\t: x86_64\n");

        // /home/root
        layer.insert("/home/root/.profile",   b"# OCI Kernel root profile\n");
        layer.insert("/home/root/readme.txt",
            b"OCI Kernel 0.1.0 - a Rust bare-metal OCI container runtime kernel.\n\
              Type 'help' for shell commands.\n");

        // /var/log
        layer.insert("/var/log/kernel.log",   b"[boot] OCI Kernel 0.1.0 initialized\n");

        // Directory markers — hidden .keep files make empty dirs discoverable
        // without showing up in ls output (filtered by the leading dot).
        layer.insert("/dev/.keep",  b"");
        layer.insert("/tmp/.keep",  b"");
        layer.insert("/bin/.keep",  b"");
        layer.insert("/var/.keep",  b"");
        layer.insert("/var/log/.keep", b"");

        Self::new(alloc::vec![Arc::new(layer)])
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
            if let Some(existing) = self.read_lower(path) {
                self.upper.write(path, &existing);
            }
        }
        self.upper.write(path, data);
        Ok(())
    }

    /// Remove a file from the upper (writable) layer.
    /// Returns PermissionDenied if the file only exists in a read-only lower layer.
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        if self.upper.files.remove(path).is_some() {
            return Ok(());
        }
        if self.lower.iter().any(|l| l.exists(path)) {
            return Err(FsError::PermissionDenied);
        }
        Err(FsError::NotFound)
    }

    /// Check if a path exists in any layer.
    pub fn exists(&self, path: &str) -> bool {
        if self.upper.exists(path) {
            return true;
        }
        self.lower.iter().any(|l| l.exists(path))
    }

    /// True if `path` is a directory (i.e. any stored path starts with `path/`).
    pub fn is_dir(&self, path: &str) -> bool {
        if path == "/" { return true; }
        let prefix = alloc::format!("{}/", path.trim_end_matches('/'));
        self.upper.files.keys().any(|k| k.starts_with(prefix.as_str()))
            || self.lower.iter().any(|l| {
                l.files.keys().any(|k| k.starts_with(prefix.as_str()))
            })
    }

    /// List direct children of `dir`.
    ///
    /// Returns bare filenames for files and `"name/"` for subdirectories.
    /// Hidden entries (names starting with `.`) are omitted from output.
    pub fn list(&self, dir: &str) -> Vec<String> {
        use alloc::collections::BTreeSet;

        let prefix = if dir == "/" {
            String::from("/")
        } else {
            alloc::format!("{}/", dir.trim_end_matches('/'))
        };

        // Collect every path across all layers.
        let mut all: Vec<String> = self.upper.files.keys().cloned().collect();
        for layer in &self.lower {
            for k in layer.files.keys() {
                all.push(k.clone());
            }
        }

        let mut seen: BTreeSet<String> = BTreeSet::new();

        for path in &all {
            let rel = match path.strip_prefix(prefix.as_str()) {
                Some(r) if !r.is_empty() => r,
                _ => continue,
            };

            // Skip hidden marker files (e.g. .keep, .profile).
            if rel.starts_with('.') { continue; }

            let component = match rel.split('/').next() {
                Some(c) if !c.is_empty() => c,
                _ => continue,
            };

            // If any stored path continues past this component it is a directory.
            let subdir = alloc::format!("{}{}/", prefix, component);
            if all.iter().any(|p| p.starts_with(subdir.as_str())) {
                seen.insert(alloc::format!("{}/", component));
            } else {
                seen.insert(String::from(component));
            }
        }

        seen.into_iter().collect()
    }

    /// True if the path exists in the upper (writable) layer.
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
    pub fn new_test() -> Self {
        Self::new(alloc::vec![Arc::new(MemLayer::new())])
    }

    pub fn lower_write(&mut self, path: &str, data: &[u8]) {
        Arc::get_mut(&mut self.lower[0])
            .expect("lower layer shared — can't mutate in test")
            .insert(path, data);
    }

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
        assert_eq!(overlay.lower_read("/etc/hosts"), b"original");
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
        assert!(overlay.upper_exists("/file"));
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

    #[test]
    fn remove_upper_file() {
        let mut overlay = OverlayMount::new_test();
        overlay.write("/tmp/f", b"data").unwrap();
        assert!(overlay.remove("/tmp/f").is_ok());
        assert_eq!(overlay.read("/tmp/f"), Err(FsError::NotFound));
    }

    #[test]
    fn remove_lower_file_returns_permission_denied() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hosts", b"data");
        assert_eq!(overlay.remove("/etc/hosts"), Err(FsError::PermissionDenied));
    }

    #[test]
    fn list_directory() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hostname", b"host");
        overlay.lower_write("/etc/motd",     b"motd");
        overlay.lower_write("/etc/sub/file", b"sub");

        let mut entries = overlay.list("/etc");
        entries.sort();
        assert!(entries.contains(&"hostname".to_string()));
        assert!(entries.contains(&"motd".to_string()));
        assert!(entries.contains(&"sub/".to_string()));
    }

    #[test]
    fn list_root() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hostname", b"host");
        overlay.lower_write("/home/user/file", b"data");

        let entries = overlay.list("/");
        assert!(entries.contains(&"etc/".to_string()));
        assert!(entries.contains(&"home/".to_string()));
    }

    #[test]
    fn is_dir_recognises_directories() {
        let mut overlay = OverlayMount::new_test();
        overlay.lower_write("/etc/hostname", b"host");

        assert!(overlay.is_dir("/"));
        assert!(overlay.is_dir("/etc"));
        assert!(!overlay.is_dir("/etc/hostname")); // that's a file
        assert!(!overlay.is_dir("/nonexistent"));
    }

    #[test]
    fn new_kernel_root_has_expected_files() {
        let fs = OverlayMount::new_kernel_root();
        assert!(fs.exists("/etc/hostname"));
        assert!(fs.exists("/etc/os-release"));
        assert!(fs.is_dir("/etc"));
        assert!(fs.is_dir("/home/root"));
        assert!(fs.is_dir("/tmp"));
    }
}
