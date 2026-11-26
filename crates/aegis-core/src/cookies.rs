//! Cookie storage with First-Party Isolation.
//!
//! The storage key is `(container, registrable domain)`. Two containers never
//! see each other's cookies, and by default a third-party request receives no
//! cookies at all, even inside the same container.

use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use url::Url;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// `None` means a session cookie: kept in memory, never written to disk.
    pub expires: Option<i64>,
    pub host_only: bool,
    pub same_site: String,
    pub created_at: i64,
}

impl Cookie {
    fn is_expired(&self, now: i64) -> bool {
        self.expires.map(|e| e <= now).unwrap_or(false)
    }

    fn domain_matches(&self, host: &str) -> bool {
        if self.host_only {
            host.eq_ignore_ascii_case(&self.domain)
        } else {
            host.eq_ignore_ascii_case(&self.domain)
                || host
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", self.domain.to_ascii_lowercase()))
        }
    }

    fn path_matches(&self, request_path: &str) -> bool {
        if self.path == "/" {
            return true;
        }
        request_path == self.path
            || (request_path.starts_with(&self.path)
                && (self.path.ends_with('/')
                    || request_path.as_bytes().get(self.path.len()) == Some(&b'/')))
    }
}

#[derive(Default)]
pub struct CookieJar {
    inner: Mutex<HashMap<String, Vec<Cookie>>>,
    /// Number of cookies withheld because the request was third-party.
    pub withheld: std::sync::atomic::AtomicU64,
}

fn key(container: &str) -> String {
    container.to_string()
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store_from_response(
        &self,
        container: &str,
        url: &Url,
        set_cookies: &[String],
        now: i64,
    ) -> usize {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return 0,
        };
        let default_path = default_path(url);
        let mut stored = 0;
        let mut map = self.inner.lock().unwrap();
        let bucket = map.entry(key(container)).or_default();
        for raw in set_cookies {
            let parsed = parse_set_cookie(raw, &host, &default_path, now);
            if let Some(cookie) = parsed {
                bucket.retain(|c| {
                    !(c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path)
                });
                bucket.push(cookie);
                stored += 1;
            }
        }
        stored
    }

    /// Header value for a request, or `None`.
    pub fn header_for(
        &self,
        container: &str,
        url: &Url,
        top_site: Option<&str>,
        allow_third_party: bool,
        now: i64,
    ) -> Option<String> {
        let host = url.host_str()?.to_ascii_lowercase();
        let path = if url.path().is_empty() { "/" } else { url.path() };
        let secure = url.scheme() == "https";

        let third_party = match top_site {
            Some(top) => crate::filter::is_third_party(&host, top),
            None => false,
        };
        if third_party && !allow_third_party {
            return None;
        }

        let mut map = self.inner.lock().unwrap();
        let bucket = map.entry(key(container)).or_default();
        let before = bucket.len();
        bucket.retain(|c| !c.is_expired(now));
        let _ = before;

        let mut matches: Vec<&Cookie> = bucket
            .iter()
            .filter(|c| {
                c.domain_matches(&host)
                    && c.path_matches(path)
                    && (!c.secure || secure)
                    && !(c.same_site == "strict" && third_party)
                    && !(c.same_site == "lax" && third_party)
            })
            .collect();
        if matches.is_empty() {
            return None;
        }
        matches.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        let header = matches
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");
        Some(header)
    }

    pub fn count(&self) -> usize {
        self.inner.lock().unwrap().values().map(|v| v.len()).sum()
    }

    pub fn count_for(&self, container: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .get(container)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn clear_container(&self, container: &str) {
        self.inner.lock().unwrap().remove(container);
    }

    pub fn clear_all(&self) {
        self.inner.lock().unwrap().clear();
    }

    pub fn load(&self, path: &Path) {
        if let Some(saved) = util::read_json::<HashMap<String, Vec<Cookie>>>(path) {
            let now = util::now_ts();
            let mut map = self.inner.lock().unwrap();
            for (container, cookies) in saved {
                let entry = map.entry(container).or_default();
                for c in cookies {
                    if c.expires.map(|e| e > now).unwrap_or(false) {
                        entry.push(c);
                    }
                }
            }
        }
    }

    pub fn save(&self, path: &Path) {
        let now = util::now_ts();
        let map = self.inner.lock().unwrap();
        let persisted: HashMap<String, Vec<Cookie>> = map
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.iter()
                        .filter(|c| c.expires.map(|e| e > now).unwrap_or(false))
                        .cloned()
                        .collect(),
                )
            })
            .filter(|(_, v): &(String, Vec<Cookie>)| !v.is_empty())
            .collect();
        let _ = util::write_json(path, &persisted);
    }
}

fn default_path(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() || path == "/" {
        return "/".into();
    }
    match path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

fn parse_set_cookie(raw: &str, host: &str, default_path: &str, now: i64) -> Option<Cookie> {
    let mut parts = raw.split(';');
    let pair = parts.next()?.trim();
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut cookie = Cookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        domain: host.to_string(),
        path: default_path.to_string(),
        secure: false,
        http_only: false,
        expires: None,
        host_only: true,
        same_site: "lax".into(),
        created_at: now,
    };
    let mut max_age: Option<i64> = None;
    let mut expires: Option<i64> = None;
    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (k, v) = match attr.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim()),
            None => (attr.to_ascii_lowercase(), ""),
        };
        match k.as_str() {
            "domain" => {
                let d = v.trim_start_matches('.').to_ascii_lowercase();
                if !d.is_empty()
                    && (host == d || host.ends_with(&format!(".{d}")))
                {
                    cookie.domain = d;
                    cookie.host_only = false;
                }
            }
            "path" => {
                if v.starts_with('/') {
                    cookie.path = v.to_string();
                }
            }
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            "samesite" => match v.to_ascii_lowercase().as_str() {
                "strict" => cookie.same_site = "strict".into(),
                "none" => cookie.same_site = "none".into(),
                _ => cookie.same_site = "lax".into(),
            },
            "max-age" => max_age = v.parse::<i64>().ok(),
            "expires" => expires = parse_http_date(v),
            _ => {}
        }
    }
    cookie.expires = match max_age {
        Some(0) => return None,
        Some(secs) => Some(now + secs),
        None => expires,
    };
    Some(cookie)
}

fn parse_http_date(raw: &str) -> Option<i64> {
    // "Wed, 21 Oct 2015 07:28:00 GMT"
    let cleaned = raw.replace(',', " ");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let (day, month, year, time) = if parts[0].chars().all(|c| c.is_ascii_digit()) {
        (parts[0], parts[1], parts[2], parts[3])
    } else {
        (parts[1], parts[2], parts[3], parts[4])
    };
    let day: u32 = day.parse().ok()?;
    let year: i32 = year.parse().ok()?;
    let month = match month.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let mut hms = time.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next().unwrap_or("0").parse().ok()?;
    let s: i64 = hms.next().unwrap_or("0").parse().ok()?;
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads() {
        let jar = CookieJar::new();
        let url = Url::parse("https://shop.example.com/cart/items").unwrap();
        jar.store_from_response(
            "personal",
            &url,
            &[
                "sid=abc; Path=/; Secure; HttpOnly".to_string(),
                "pref=dark; Path=/cart; Max-Age=3600".to_string(),
            ],
            1000,
        );
        let header = jar
            .header_for("personal", &url, Some("example.com"), true, 1000)
            .unwrap();
        assert!(header.contains("sid=abc"));
        assert!(header.contains("pref=dark"));
        // HTTP request: secure cookie must not be sent.
        let plain = Url::parse("http://shop.example.com/cart").unwrap();
        let header = jar
            .header_for("personal", &plain, Some("example.com"), true, 1000)
            .unwrap();
        assert!(!header.contains("sid=abc"));
    }

    #[test]
    fn containers_are_isolated() {
        let jar = CookieJar::new();
        let url = Url::parse("https://mail.example.com/").unwrap();
        jar.store_from_response("personal", &url, &["a=1".into()], 10);
        assert!(jar.header_for("work", &url, None, true, 10).is_none());
    }

    #[test]
    fn third_party_blocked_by_default() {
        let jar = CookieJar::new();
        let url = Url::parse("https://ads.tracker.net/pixel").unwrap();
        jar.store_from_response(
            "personal",
            &url,
            &["id=9; SameSite=None; Secure".into()],
            10,
        );
        assert!(jar
            .header_for("personal", &url, Some("example.com"), false, 10)
            .is_none());
        assert!(jar
            .header_for("personal", &url, Some("example.com"), true, 10)
            .is_some());
    }

    #[test]
    fn parses_http_date() {
        let ts = parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        let (y, m, d, h, min) = crate::util::civil_from_unix(ts);
        assert_eq!((y, m, d, h, min), (2015, 10, 21, 7, 28));
    }
}
