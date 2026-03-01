extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::borrow::ToOwned;

use super::tls;

#[derive(Debug)]
pub struct HttpResponse {
    pub status:  u16,
    pub headers: Vec<(String, String)>,
    pub body:    Vec<u8>,
}

#[derive(Debug)]
pub enum HttpError {
    Network(tls::TlsConnectError),
    InvalidResponse,
    InvalidStatus,
    InvalidChunk,
}

impl From<tls::TlsConnectError> for HttpError {
    fn from(e: tls::TlsConnectError) -> Self {
        HttpError::Network(e)
    }
}

/// Perform an HTTP/1.1 GET request over HTTPS.
pub fn get(host: &str, path: &str) -> Result<HttpResponse, HttpError> {
    get_with_headers(host, path, &[])
}

/// Perform an HTTP/1.1 GET with additional request headers.
pub fn get_with_headers(
    host: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
) -> Result<HttpResponse, HttpError> {
    let mut req = alloc::format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: oci-kernel/0.1\r\n",
        path, host
    );
    for (k, v) in extra_headers {
        req.push_str(&alloc::format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");

    let raw = tls::https_request(host, 443, req.as_bytes())?;
    parse_response(&raw)
}

/// Parse a complete HTTP/1.1 response from raw bytes.
pub fn parse_response(raw: &[u8]) -> Result<HttpResponse, HttpError> {
    // Find the header/body separator (\r\n\r\n).
    let sep = find_header_end(raw).ok_or(HttpError::InvalidResponse)?;
    let header_section = &raw[..sep];
    let body_raw = &raw[sep + 4..]; // skip \r\n\r\n

    // Parse status line.
    let header_str = core::str::from_utf8(header_section)
        .map_err(|_| HttpError::InvalidResponse)?;
    let mut lines = header_str.split("\r\n");

    let status_line = lines.next().ok_or(HttpError::InvalidStatus)?;
    let status = parse_status(status_line)?;

    // Parse headers.
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let name  = line[..colon].trim().to_owned();
            let value = line[colon + 1..].trim().to_owned();
            headers.push((name, value));
        }
    }

    // Determine body transfer encoding.
    let is_chunked = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("transfer-encoding")
            && v.trim().eq_ignore_ascii_case("chunked")
    });

    let body = if is_chunked {
        parse_chunked(body_raw)?
    } else {
        // Use Content-Length if present; otherwise take all remaining bytes.
        if let Some((_, v)) = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            let len: usize = v.trim().parse().map_err(|_| HttpError::InvalidResponse)?;
            body_raw[..len.min(body_raw.len())].to_vec()
        } else {
            body_raw.to_vec()
        }
    };

    Ok(HttpResponse { status, headers, body })
}

/// Decode a chunked transfer-encoding body.
pub fn parse_chunked(mut data: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut result = Vec::new();
    loop {
        // Read the chunk-size line (terminated by CRLF).
        let crlf = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(HttpError::InvalidChunk)?;
        let size_str = core::str::from_utf8(&data[..crlf])
            .map_err(|_| HttpError::InvalidChunk)?;
        // Strip optional chunk extensions (after a semicolon).
        let size_str = size_str.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_str, 16)
            .map_err(|_| HttpError::InvalidChunk)?;
        data = &data[crlf + 2..];

        if chunk_size == 0 {
            break; // Last chunk.
        }

        if data.len() < chunk_size + 2 {
            return Err(HttpError::InvalidChunk);
        }
        result.extend_from_slice(&data[..chunk_size]);
        data = &data[chunk_size + 2..]; // skip trailing \r\n
    }
    Ok(result)
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_status(line: &str) -> Result<u16, HttpError> {
    // Handles "HTTP/1.1 200 OK" and "HTTP/2 200".
    let mut parts = line.splitn(3, ' ');
    let _version = parts.next().ok_or(HttpError::InvalidStatus)?;
    let code_str = parts.next().ok_or(HttpError::InvalidStatus)?;
    code_str.parse::<u16>().map_err(|_| HttpError::InvalidStatus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_response_status() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let resp = parse_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn parse_chunked_response() {
        let raw =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let resp = parse_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn parse_header_extraction() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\nWww-Authenticate: Bearer realm=\"https://auth.docker.io/token\"\r\nContent-Length: 0\r\n\r\n";
        let resp = parse_response(raw).unwrap();
        assert_eq!(resp.status, 401);
        let auth = resp
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("www-authenticate"));
        assert!(auth.is_some());
    }

    #[test]
    fn parse_chunked_multiple_chunks() {
        let raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let body = parse_chunked(raw).unwrap();
        assert_eq!(body, b"hello world");
    }
}
