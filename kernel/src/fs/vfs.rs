extern crate alloc;
use alloc::vec::Vec;

/// Minimal VFS errors.
#[derive(Debug, PartialEq)]
pub enum FsError {
    NotFound,
    PermissionDenied,
    AlreadyExists,
}

/// Core VFS trait — anything that can be read/written by path.
pub trait Filesystem {
    fn read(&self, path: &str) -> Result<Vec<u8>, FsError>;
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError>;
    fn exists(&self, path: &str) -> bool;
}
