//! Local filtering proxy for the compatibility webview.
//!
//! The webview asks this proxy for every document and subresource. We only
//! inspect the destination host (CONNECT target / absolute URI), so TLS stays
//! end-to-end while ad, tracker and miner hosts are refused before a byte of
//! their content reaches Chromium.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aegis_core::http::HttpClient;
use aegis_core::models::Shield;

const MAX_HEADER: usize = 32 * 1024;

pub fn start(client: Arc<HttpClient>, shield: Arc<Mutex<Shield>>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new()
        .name("rivus-proxy".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let client = client.clone();
                let shield = shield.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, client, shield);
                });
            }
        })?;
    Ok(port)
}

fn handle(mut client: TcpStream, http: Arc<HttpClient>, shield: Arc<Mutex<Shield>>) -> std::io::Result<()> {
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = client.set_write_timeout(Some(Duration::from_secs(30)));
    let _ = client.set_nodelay(true);

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = client.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > MAX_HEADER {
            return Ok(());
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let target = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || target.is_empty() {
        return Ok(());
    }

    let head_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(buf.len());

    let decision = {
        let shield = shield.lock().unwrap_or_else(|e| e.into_inner());
        match destination(&method, &target) {
            Some((host, _port)) => Some(http.filter.check(&host, None, &shield)),
            None => None,
        }
    };
    if let Some(d) = decision {
        if d.blocked {
            if let Some((host, _)) = destination(&method, &target) {
                http.block(&host, d.category.unwrap_or(aegis_core::Category::Tracker));
            }
            let body = b"Rivus Shield blocked this request";
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = client.write_all(response.as_bytes());
            let _ = client.write_all(body);
            return Ok(());
        }
    }

    if method == "CONNECT" {
        let (host, port) = destination(&method, &target).unwrap_or_default();
        let upstream = match TcpStream::connect((host.as_str(), port)) {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return Ok(());
            }
        };
        let _ = upstream.set_nodelay(true);
        let _ = upstream.set_read_timeout(Some(Duration::from_secs(120)));
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        return tunnel(client, upstream);
    }

    // Plain HTTP: the request line carries an absolute URI.
    let parsed = url::Url::parse(&target).ok();
    let (host, port) = match parsed.as_ref().and_then(|u| u.host_str().map(|h| (h.to_string(), u.port_or_known_default().unwrap_or(80)))) {
        Some(v) => v,
        None => return Ok(()),
    };
    let mut upstream = match TcpStream::connect((host.as_str(), port)) {
        Ok(s) => s,
        Err(_) => {
            let _ = client.write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return Ok(());
        }
    };
    let path = parsed
        .as_ref()
        .map(|u| {
            let mut p = u.path().to_string();
            if let Some(q) = u.query() {
                p.push('?');
                p.push_str(q);
            }
            p
        })
        .unwrap_or_else(|| "/".into());
    let mut rewritten = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    for line in text.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") {
            continue;
        }
        rewritten.extend_from_slice(line.as_bytes());
        rewritten.extend_from_slice(b"\r\n");
    }
    rewritten.extend_from_slice(b"\r\n");
    // Keep any body bytes that arrived with the header.
    rewritten.extend_from_slice(&buf[head_end..]);
    upstream.write_all(&rewritten)?;
    tunnel(client, upstream)
}

fn destination(method: &str, target: &str) -> Option<(String, u16)> {
    if method == "CONNECT" {
        let t = target.trim();
        if let Some(rest) = t.strip_prefix('[') {
            let (host, tail) = rest.split_once(']')?;
            let port = tail.trim_start_matches(':').trim();
            return Some((host.to_string(), port.parse().unwrap_or(443)));
        }
        let (host, port) = t.rsplit_once(':')?;
        if host.is_empty() {
            return None;
        }
        return Some((host.to_string(), port.parse().unwrap_or(443)));
    }
    let url = url::Url::parse(target).ok()?;
    Some((
        url.host_str()?.to_string(),
        url.port_or_known_default().unwrap_or(80),
    ))
}

fn tunnel(mut client: TcpStream, mut upstream: TcpStream) -> std::io::Result<()> {
    let mut up_read = upstream.try_clone()?;
    let mut down_write = client.try_clone()?;
    let pump = std::thread::spawn(move || {
        let _ = std::io::copy(&mut up_read, &mut down_write);
        let _ = down_write.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut client, &mut upstream);
    let _ = upstream.shutdown(std::net::Shutdown::Write);
    let _ = pump.join();
    Ok(())
}
