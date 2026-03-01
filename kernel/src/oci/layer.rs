extern crate alloc;
use alloc::vec::Vec;

#[derive(Debug)]
pub enum LayerError {
    DecompressFailed,
    UnsupportedCompression,
}

/// Decompress an OCI layer based on its media type.
/// Supports gzip (most common) and raw (uncompressed).
pub fn decompress(data: &[u8], media_type: &str) -> Result<Vec<u8>, LayerError> {
    if media_type.contains("gzip") || media_type.contains("gz") {
        // miniz_oxide: inflate raw deflate data
        // gzip format: variable header + compressed deflate data + 8-byte trailer
        // Strip the gzip envelope and decompress the raw deflate stream.
        let deflate_bytes = strip_gzip_header(data)?;
        let inflated = miniz_oxide::inflate::decompress_to_vec_with_limit(
            deflate_bytes,
            256 * 1024 * 1024, // 256MB limit per layer
        )
        .map_err(|_| LayerError::DecompressFailed)?;
        Ok(inflated)
    } else if media_type.contains("zstd") {
        // zstd not yet supported — most Docker Hub layers are gzip anyway
        Err(LayerError::UnsupportedCompression)
    } else if media_type.contains("tar") && !media_type.contains("gz") {
        // Raw uncompressed tar
        Ok(data.to_vec())
    } else {
        Err(LayerError::UnsupportedCompression)
    }
}

/// Strip the gzip header to get the raw deflate stream.
/// Also strips the 8-byte gzip trailer (CRC32 + size).
fn strip_gzip_header(data: &[u8]) -> Result<&[u8], LayerError> {
    if data.len() < 18 {
        return Err(LayerError::DecompressFailed);
    }
    // Magic bytes: 1f 8b
    if data[0] != 0x1f || data[1] != 0x8b {
        return Err(LayerError::DecompressFailed);
    }
    // Skip 10-byte fixed header + optional extra/name/comment fields
    let mut offset = 10usize;
    let flags = data[3];
    if flags & 0x04 != 0 {
        // FEXTRA
        if offset + 2 > data.len() {
            return Err(LayerError::DecompressFailed);
        }
        let xlen = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2 + xlen;
    }
    if flags & 0x08 != 0 {
        // FNAME — null-terminated
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }
        offset += 1;
    }
    if flags & 0x10 != 0 {
        // FCOMMENT — null-terminated
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }
        offset += 1;
    }
    if flags & 0x02 != 0 {
        // FHCRC
        offset += 2;
    }
    if offset >= data.len() {
        return Err(LayerError::DecompressFailed);
    }
    // Strip 8-byte trailer (CRC32 + original size)
    let end = data.len().saturating_sub(8);
    if offset > end {
        return Err(LayerError::DecompressFailed);
    }
    Ok(&data[offset..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_compression_errors() {
        let err = decompress(b"fake", "application/vnd.oci.image.layer.v1.zstd");
        assert!(err.is_err());
    }

    #[test]
    fn raw_tar_passthrough() {
        let data = b"fake tar data";
        let result = decompress(data, "application/vnd.docker.image.rootfs.diff.tar");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn bad_gzip_magic_errors() {
        let result = decompress(b"not gzip", "application/vnd.docker.image.rootfs.diff.tar.gzip");
        assert!(result.is_err());
    }
}
