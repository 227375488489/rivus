//! Minimal DNS client used only for CNAME-cloaking detection.
//!
//! Queries go to the DNS servers the operating system already uses; no
//! resolver owned by Aegis, no third party. Results are cached for 10 minutes.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct DnsResolver {
    servers: Vec<IpAddr>,
    cache: Mutex<HashMap<String, (Instant, Vec<String>)>>,
}

const CACHE_TTL: Duration = Duration::from_secs(600);
const TIMEOUT: Duration = Duration::from_millis(1200);

impl DnsResolver {
    pub fn system() -> Self {
        Self {
            servers: system_servers(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn is_available(&self) -> bool {
        !self.servers.is_empty()
    }

    /// Follow the CNAME chain of `host` and return every target, in order.
    /// Returns an empty vector when there is no CNAME or DNS is unavailable.
    pub fn cname_chain(&self, host: &str) -> Vec<String> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return Vec::new();
        }
        if let Some((at, chain)) = self.cache.lock().unwrap().get(&host) {
            if at.elapsed() < CACHE_TTL {
                return chain.clone();
            }
        }
        let mut chain: Vec<String> = Vec::new();
        let mut current = host.clone();
        for _ in 0..8 {
            let target = match self.lookup_cname(&current) {
                Some(t) => t,
                None => break,
            };
            if target.eq_ignore_ascii_case(&current) || chain.iter().any(|c| c.eq_ignore_ascii_case(&target)) {
                break;
            }
            chain.push(target.clone());
            current = target;
        }
        self.cache
            .lock()
            .unwrap()
            .insert(host, (Instant::now(), chain.clone()));
        chain
    }

    fn lookup_cname(&self, name: &str) -> Option<String> {
        let query = build_query(name, 5 /* CNAME */)?;
        for server in &self.servers {
            let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else { continue };
            socket.set_read_timeout(Some(TIMEOUT)).ok()?;
            socket.set_write_timeout(Some(TIMEOUT)).ok()?;
            let addr = SocketAddr::new(*server, 53);
            if socket.send_to(&query, addr).is_err() {
                continue;
            }
            let mut buf = [0u8; 2048];
            let Ok((n, _)) = socket.recv_from(&mut buf) else { continue };
            if n < 12 || buf[0..2] != query[0..2] {
                continue;
            }
            if let Some(target) = parse_first_cname(&buf[..n]) {
                return Some(target);
            }
            // A response with no CNAME is authoritative enough.
            return None;
        }
        None
    }
}

fn build_query(name: &str, qtype: u16) -> Option<Vec<u8>> {
    let id = rand::random::<u16>();
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes()); // recursion desired
    out.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    out.extend_from_slice(&0u16.to_be_bytes()); // ancount
    out.extend_from_slice(&0u16.to_be_bytes()); // nscount
    out.extend_from_slice(&0u16.to_be_bytes()); // arcount
    for label in name.trim_end_matches('.').split('.') {
        if label.len() > 63 || label.is_empty() {
            return None;
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // class IN
    Some(out)
}

fn read_name(msg: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    let mut jumped = false;
    let mut end = start;
    let mut hops = 0;
    loop {
        if pos >= msg.len() || hops > 16 {
            return None;
        }
        let len = msg[pos];
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= msg.len() {
                return None;
            }
            let offset = (((len as usize) & 0x3F) << 8) | msg[pos + 1] as usize;
            if !jumped {
                end = pos + 2;
                jumped = true;
            }
            pos = offset;
            hops += 1;
            continue;
        }
        if len == 0 {
            pos += 1;
            if !jumped {
                end = pos;
            }
            break;
        }
        let len = len as usize;
        if pos + 1 + len > msg.len() {
            return None;
        }
        let label = String::from_utf8_lossy(&msg[pos + 1..pos + 1 + len]).to_string();
        labels.push(label);
        pos += 1 + len;
    }
    Some((labels.join("."), end))
}

fn parse_first_cname(msg: &[u8]) -> Option<String> {
    let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qd {
        let (_, next) = read_name(msg, pos)?;
        pos = next + 4;
    }
    for _ in 0..an {
        let (_, next) = read_name(msg, pos)?;
        pos = next;
        if pos + 10 > msg.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
        let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
        let rdata = pos + 10;
        if rdata + rdlen > msg.len() {
            return None;
        }
        if rtype == 5 {
            let (target, _) = read_name(msg, rdata)?;
            return Some(target.to_ascii_lowercase());
        }
        pos = rdata + rdlen;
    }
    None
}

#[cfg(windows)]
fn system_servers() -> Vec<IpAddr> {
    use std::ffi::c_void;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_FRIENDLY_NAME,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

    let mut out = Vec::new();
    unsafe {
        let mut size: u32 = 15_000;
        let mut buffer: Vec<u8> = Vec::new();
        for _ in 0..3 {
            buffer.resize(size as usize, 0);
            let mut len = size;
            let rc = GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_FRIENDLY_NAME,
                std::ptr::null::<c_void>(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut len,
            );
            if rc == 0 {
                break;
            }
            if rc == 111 {
                // ERROR_BUFFER_OVERFLOW
                size = len;
                continue;
            }
            return out;
        }
        let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            let mut dns = (*adapter).FirstDnsServerAddress;
            while !dns.is_null() {
                let sa = (*dns).Address;
                let p = &sa as *const _ as *const u8;
                let family = *(p as *const u16);
                if family == AF_INET as u16 {
                    let octets = [*p.add(4), *p.add(5), *p.add(6), *p.add(7)];
                    out.push(IpAddr::from(octets));
                } else if family == AF_INET6 as u16 {
                    let mut octets = [0u8; 16];
                    for (i, b) in octets.iter_mut().enumerate() {
                        *b = *p.add(8 + i);
                    }
                    out.push(IpAddr::from(octets));
                }
                dns = (*dns).Next;
            }
            adapter = (*adapter).Next;
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(not(windows))]
fn system_servers() -> Vec<IpAddr> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("nameserver") {
                if let Ok(ip) = rest.trim().parse::<IpAddr>() {
                    out.push(ip);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_valid_query() {
        let q = build_query("metrics.example.com", 5).unwrap();
        assert_eq!(q[2] & 0x01, 0x01);
        assert!(q.len() > 20);
    }

    #[test]
    fn reads_compressed_names() {
        // Header with one CNAME answer using a compression pointer.
        let mut msg = vec![0u8; 12];
        msg[4] = 0;
        msg[5] = 1; // qd
        msg[6] = 0;
        msg[7] = 1; // an
        msg.extend_from_slice(&[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]);
        msg.extend_from_slice(&[0, 5, 0, 1]); // CNAME IN
        // answer: name pointer to 12
        msg.extend_from_slice(&[0xC0, 12]);
        msg.extend_from_slice(&[0, 5, 0, 1]);
        msg.extend_from_slice(&[0, 0, 0, 60]);
        let rdata_at = msg.len() + 2;
        let mut rdata = vec![3, b'a', b'd', b's', 0xC0, 12];
        msg.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        msg.append(&mut rdata);
        let target = parse_first_cname(&msg).unwrap();
        assert_eq!(target, "ads.example.com");
        let _ = rdata_at;
    }
}
