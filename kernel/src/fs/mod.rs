pub mod vfs;
pub mod overlayfs;

/// Kernel root filesystem — shared across all shell sessions.
/// Pre-populated with /etc, /home/root, /tmp, /proc, /dev, /var/log.
/// Writable: all writes go to the upper overlay layer (in-memory).
#[cfg(not(test))]
pub static ROOT_FS: spin::Lazy<spin::Mutex<overlayfs::OverlayMount>> =
    spin::Lazy::new(|| spin::Mutex::new(overlayfs::OverlayMount::new_kernel_root()));
