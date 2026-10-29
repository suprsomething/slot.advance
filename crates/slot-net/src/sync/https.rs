//! A small HTTPS client for Syncthing's own web interface (P2-1, approved 25 October 2026).
//!
//! - Talks only to the handheld itself (loopback, or the configured address).
//! - Trusts exactly one certificate: the one Syncthing generated for its web interface
//!   (`config/https-cert.pem`), read from disk for every request, so a certificate
//!   Syncthing regenerates is picked up and nothing else is ever accepted. Verification is
//!   never switched off: the server must also prove it holds that certificate's key.
//! - HTTP/1.1 with `Connection: close`, bounded in time and size. The API key goes only in
//!   the `X-API-Key` header, only over this TLS connection.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

pub const CONNECT_LIMIT: Duration = Duration::from_secs(2);
pub const IO_LIMIT: Duration = Duration::from_secs(5);
const MAX_RESPONSE: usize = 1 << 20;

#[derive(Debug)]
pub enum Error {
    /// Nothing listening, or not reachable.
    Unreachable(String),
    /// The certificate file could not be read.
    NoCertificate(String),
    /// The server's certificate is not Syncthing's web interface certificate.
    WrongCertificate,
    /// The TLS handshake failed for another reason (for example the port speaks plain HTTP).
    Tls(String),
    Timeout,
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable(e) => write!(f, "not reachable ({e})"),
            Error::NoCertificate(e) => write!(f, "web interface certificate unreadable ({e})"),
            Error::WrongCertificate => write!(f, "not Syncthing's web interface certificate"),
            Error::Tls(e) => write!(f, "TLS failed ({e})"),
            Error::Timeout => write!(f, "timed out"),
            Error::Protocol(e) => write!(f, "unexpected answer ({e})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// Accepts only the pinned certificate, and checks the handshake signature with it.
#[derive(Debug)]
struct Pinned {
    cert: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.cert.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("not the pinned certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Where Syncthing's web interface is, and which certificate it must present.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub addr: SocketAddr,
    pub cert: PathBuf,
    pub connect_limit: Duration,
    pub io_limit: Duration,
}

impl Endpoint {
    pub fn new(addr: SocketAddr, cert: &Path) -> Endpoint {
        Endpoint {
            addr,
            cert: cert.to_path_buf(),
            connect_limit: CONNECT_LIMIT,
            io_limit: IO_LIMIT,
        }
    }

    /// The same endpoint with shorter limits (the shutdown request must fit its stop stage).
    pub fn with_limits(&self, connect: Duration, io: Duration) -> Endpoint {
        Endpoint {
            connect_limit: connect,
            io_limit: io,
            ..self.clone()
        }
    }

    fn config(&self) -> Result<Arc<rustls::ClientConfig>, Error> {
        let der = CertificateDer::from_pem_file(&self.cert)
            .map_err(|e| Error::NoCertificate(e.to_string()))?;
        let provider = Arc::new(ring::default_provider());
        let verifier = Arc::new(Pinned {
            cert: der.as_ref().to_vec(),
            provider: provider.clone(),
        });
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| Error::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        Ok(Arc::new(config))
    }

    fn connect(&self) -> Result<TcpStream, Error> {
        let tcp = TcpStream::connect_timeout(&self.addr, self.connect_limit).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                Error::Timeout
            } else {
                Error::Unreachable(e.to_string())
            }
        })?;
        let _ = tcp.set_read_timeout(Some(self.io_limit));
        let _ = tcp.set_write_timeout(Some(self.io_limit));
        let _ = tcp.set_nodelay(true);
        Ok(tcp)
    }

    /// One request. `key` is the API key (none for `/rest/noauth/...`).
    pub fn request(
        &self,
        method: &str,
        path: &str,
        key: Option<&str>,
        body: Option<&str>,
    ) -> Result<Response, Error> {
        let config = self.config()?;
        let name = ServerName::IpAddress(self.addr.ip().into());
        let conn =
            rustls::ClientConnection::new(config, name).map_err(|e| Error::Tls(e.to_string()))?;
        let mut tls = rustls::StreamOwned::new(conn, self.connect()?);
        let req = build_request(method, path, &self.addr, key, body);
        tls.write_all(req.as_bytes()).map_err(io_error)?;
        tls.flush().map_err(io_error)?;
        let raw = read_response(&mut tls)?;
        parse_response(&raw)
    }

    /// For telling "speaks plain HTTP" (the web interface's HTTPS was switched off) from
    /// "not answering": the health endpoint needs no key, so nothing secret is sent.
    pub fn plain_health(&self) -> bool {
        let Ok(mut tcp) = self.connect() else {
            return false;
        };
        let req = build_request("GET", "/rest/noauth/health", &self.addr, None, None);
        if tcp.write_all(req.as_bytes()).is_err() {
            return false;
        }
        match read_response(&mut tcp).and_then(|r| parse_response(&r)) {
            Ok(r) => r.status == 200 && r.body.contains("OK"),
            Err(_) => false,
        }
    }
}

fn io_error(e: std::io::Error) -> Error {
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => Error::Timeout,
        std::io::ErrorKind::InvalidData => {
            let msg = e.to_string();
            if msg.contains("pinned") || msg.contains("certificate") {
                Error::WrongCertificate
            } else {
                Error::Tls(msg)
            }
        }
        _ => Error::Unreachable(e.to_string()),
    }
}

fn build_request(
    method: &str,
    path: &str,
    addr: &SocketAddr,
    key: Option<&str>,
    body: Option<&str>,
) -> String {
    let mut r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: slot.advance\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(k) = key {
        r.push_str(&format!("X-API-Key: {k}\r\n"));
    }
    match body {
        Some(b) => r.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        )),
        None if method == "POST" => r.push_str("Content-Length: 0\r\n\r\n"),
        None => r.push_str("\r\n"),
    }
    r
}

/// Reads until the response is complete (by length, chunk terminator or end of stream).
fn read_response(s: &mut impl Read) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if response_complete(&buf) {
            return Ok(buf);
        }
        match s.read(&mut chunk) {
            Ok(0) => return Ok(buf),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_RESPONSE {
                    return Err(Error::Protocol("response too large".into()));
                }
            }
            // A peer that closes without TLS close_notify: what arrived is the answer.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !buf.is_empty() => {
                return Ok(buf)
            }
            Err(e) => return Err(io_error(e)),
        }
    }
}

fn split_head(raw: &[u8]) -> Option<(&str, &[u8])> {
    let i = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..i]).ok()?;
    Some((head, &raw[i + 4..]))
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn response_complete(raw: &[u8]) -> bool {
    let Some((head, body)) = split_head(raw) else {
        return false;
    };
    if let Some(len) = header(head, "Content-Length").and_then(|v| v.parse::<usize>().ok()) {
        return body.len() >= len;
    }
    if header(head, "Transfer-Encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return dechunk(body).is_some();
    }
    // 204 and 304 carry no body.
    head.starts_with("HTTP/1.1 204") || head.starts_with("HTTP/1.1 304")
}

/// The body of a complete chunked message, or `None` if more is needed.
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&body[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        if body.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

fn parse_response(raw: &[u8]) -> Result<Response, Error> {
    let (head, body) = split_head(raw).ok_or_else(|| Error::Protocol("no headers".into()))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| Error::Protocol("no status".into()))?;
    let body = if header(head, "Transfer-Encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        dechunk(body).ok_or_else(|| Error::Protocol("incomplete chunked body".into()))?
    } else if let Some(len) = header(head, "Content-Length").and_then(|v| v.parse::<usize>().ok()) {
        body.get(..len)
            .ok_or_else(|| Error::Protocol("short body".into()))?
            .to_vec()
    } else {
        body.to_vec()
    };
    Ok(Response {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_carry_the_key_only_when_given() {
        let a: SocketAddr = "127.0.0.1:8384".parse().unwrap();
        let r = build_request("GET", "/rest/noauth/health", &a, None, None);
        assert!(r.starts_with("GET /rest/noauth/health HTTP/1.1\r\nHost: 127.0.0.1:8384\r\n"));
        assert!(!r.contains("X-API-Key"));
        assert!(r.ends_with("\r\n\r\n"));
        let r = build_request(
            "PATCH",
            "/rest/config/gui",
            &a,
            Some("K"),
            Some("{\"x\":1}"),
        );
        assert!(r.contains("X-API-Key: K\r\n"));
        assert!(r.ends_with("Content-Length: 7\r\n\r\n{\"x\":1}"));
        let r = build_request("POST", "/rest/system/shutdown", &a, Some("K"), None);
        assert!(r.ends_with("Content-Length: 0\r\n\r\n"));
    }

    #[test]
    fn responses_by_length_chunked_and_empty() {
        let by_len = b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"status\":\"OK\"}";
        assert!(response_complete(by_len));
        assert!(!response_complete(&by_len[..by_len.len() - 1]));
        assert_eq!(
            parse_response(by_len).unwrap(),
            Response {
                status: 200,
                body: "{\"status\":\"OK\"}".into()
            }
        );
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert!(response_complete(chunked));
        assert!(!response_complete(&chunked[..chunked.len() - 5]));
        assert_eq!(parse_response(chunked).unwrap().body, "{\"a\":1}");
        let none = b"HTTP/1.1 204 No Content\r\nDate: x\r\n\r\n";
        assert!(response_complete(none));
        assert_eq!(parse_response(none).unwrap().status, 204);
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn a_missing_certificate_is_reported_before_connecting() {
        let e = Endpoint::new(
            "127.0.0.1:9".parse().unwrap(),
            Path::new("/nonexistent/https-cert.pem"),
        );
        assert!(matches!(
            e.request("GET", "/rest/noauth/health", None, None),
            Err(Error::NoCertificate(_))
        ));
    }
}
