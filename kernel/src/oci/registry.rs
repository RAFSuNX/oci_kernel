extern crate alloc;
use alloc::{string::String, vec::Vec, format};

// Network stack is only available in the real kernel, not during host unit tests.
#[cfg(not(test))]
use crate::net::http::{self, HttpError};
use super::manifest::{ImageManifest, ParseError};

#[derive(Debug)]
pub enum RegistryError {
    #[cfg(not(test))]
    Http(HttpError),
    Parse(ParseError),
    Auth,
    DigestMismatch,
    Utf8,
}

#[cfg(not(test))]
impl From<HttpError> for RegistryError {
    fn from(e: HttpError) -> Self { RegistryError::Http(e) }
}

impl From<ParseError> for RegistryError {
    fn from(e: ParseError) -> Self { RegistryError::Parse(e) }
}

#[cfg(not(test))]
pub struct Registry {
    pub host:  String,
    token: Option<String>,
}

#[cfg(not(test))]
impl Registry {
    pub fn new(host: &str) -> Self {
        Self { host: host.into(), token: None }
    }

    /// Authenticate with Docker Hub to get a pull token for `image` (e.g. "library/nginx").
    pub fn authenticate(&mut self, image: &str) -> Result<(), RegistryError> {
        // Docker Hub token endpoint
        let path = format!(
            "/token?service=registry.docker.io&scope=repository:{}:pull",
            image
        );
        let resp = http::get("auth.docker.io", &path)?;
        let body_str = core::str::from_utf8(&resp.body).map_err(|_| RegistryError::Utf8)?;
        let v: serde_json::Value = serde_json::from_str(body_str)
            .map_err(|e| RegistryError::Parse(ParseError::Json(e)))?;
        let token = v["token"].as_str().ok_or(RegistryError::Auth)?;
        self.token = Some(token.into());
        Ok(())
    }

    /// Fetch the image manifest for `image:tag`.
    pub fn fetch_manifest(&self, image: &str, tag: &str) -> Result<ImageManifest, RegistryError> {
        let path = format!("/v2/{}/manifests/{}", image, tag);
        let auth_header;
        let mut extra: alloc::vec::Vec<(&str, &str)> = alloc::vec![
            ("Accept", "application/vnd.docker.distribution.manifest.v2+json"),
        ];
        if let Some(t) = &self.token {
            auth_header = alloc::format!("Bearer {}", t);
            extra.push(("Authorization", &auth_header));
        }
        let resp = http::get_with_headers(&self.host, &path, &extra)?;
        let body_str = core::str::from_utf8(&resp.body).map_err(|_| RegistryError::Utf8)?;
        ImageManifest::from_json(body_str).map_err(RegistryError::Parse)
    }

    /// Pull a single layer blob by digest. Returns the raw (compressed) bytes.
    pub fn pull_layer(&self, image: &str, digest: &str) -> Result<Vec<u8>, RegistryError> {
        let path = format!("/v2/{}/blobs/{}", image, digest);
        let resp = if let Some(t) = &self.token {
            let auth = alloc::format!("Bearer {}", t);
            http::get_with_headers(
                &self.host,
                &path,
                &[("Authorization", &auth)],
            )?
        } else {
            http::get(&self.host, &path)?
        };

        // Verify SHA256 digest
        verify_sha256(&resp.body, digest)?;
        Ok(resp.body)
    }

    /// Convenience: authenticate then fetch manifest.
    pub fn pull_manifest(
        &mut self,
        image: &str,
        tag: &str,
    ) -> Result<ImageManifest, RegistryError> {
        self.authenticate(image)?;
        self.fetch_manifest(image, tag)
    }
}

/// Verify that `data` matches `expected_digest` (format: "sha256:<hex>").
pub fn verify_sha256(data: &[u8], expected_digest: &str) -> Result<(), RegistryError> {
    let hex = expected_digest
        .strip_prefix("sha256:")
        .ok_or(RegistryError::DigestMismatch)?;

    let computed = sha256(data);
    let computed_hex = hex_encode(&computed);

    if computed_hex.as_str() != hex {
        Err(RegistryError::DigestMismatch)
    } else {
        Ok(())
    }
}

/// Minimal SHA-256 implementation (pure Rust, no_std).
/// Uses the standard SHA-256 constants and compression function.
fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of sqrt of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    // Round constants
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    // Pre-processing: add padding
    let msg_len = data.len();
    let bit_len = (msg_len as u64) * 8;

    // Number of 512-bit (64-byte) blocks
    let padded_len = ((msg_len + 9 + 63) / 64) * 64;
    let mut padded = alloc::vec![0u8; padded_len];
    padded[..msg_len].copy_from_slice(data);
    padded[msg_len] = 0x80;
    // Append bit length as big-endian u64
    let len_bytes = bit_len.to_be_bytes();
    padded[padded_len - 8..].copy_from_slice(&len_bytes);

    // Process each 512-bit block
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i*4], block[i*4+1], block[i*4+2], block[i*4+3]]);
        }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] =
            [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]];
        for i in 0..64 {
            let s1    = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch    = (e & f) ^ (!e & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0    = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj   = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, &hi) in h.iter().enumerate() {
        out[i*4..(i+1)*4].copy_from_slice(&hi.to_be_bytes());
    }
    out
}

fn hex_encode(bytes: &[u8]) -> alloc::string::String {
    use core::fmt::Write;
    let mut s = alloc::string::String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        write!(s, "{:02x}", b).ok();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty() {
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256(b"");
        let hex = hex_encode(&hash);
        assert_eq!(hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn sha256_hello() {
        // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let hash = sha256(b"hello");
        let hex = hex_encode(&hash);
        assert_eq!(hex, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn verify_digest_ok() {
        let data = b"hello";
        let digest = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify_sha256(data, digest).is_ok());
    }

    #[test]
    fn verify_digest_mismatch() {
        let data = b"hello";
        let digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
        assert!(verify_sha256(data, digest).is_err());
    }
}
