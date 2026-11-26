//! The only way Aegis reaches the network.
//!
//! Every navigation and every subresource goes through `fetch`, which checks
//! the blocklists, resolves suspicious first-party CNAMEs, enforces the cookie
//! policy and only then opens a connection.

use crate::cookies::CookieJar;
use crate::dns::DnsResolver;
use crate::filter::{is_third_party, registrable_domain, Category, FilterEngine};
use crate::models::{ReferrerPolicy, Shield, ShieldStats};
use crate::urlclean;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

/// One generic desktop Chrome identity. Every Aegis user looks identical.
pub const UNIFORM_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const AEGIS_UA: &str = concat!("Aegis/", env!("CARGO_PKG_VERSION"), " (Rust; +https://github.com/227375488489/aegis-browser)");

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub container: String,
    pub top_site: Option<Url>,
    pub shield: Shield,
    pub accept: String,
    pub max_bytes: usize,
}

impl RequestContext {
    pub fn page(container: impl Into<String>, shield: Shield) -> Self {
        Self {
            container: container.into(),
            top_site: None,
            shield,
            accept: "text/html,application/xhtml+xml,application/xml;q=0.9,text/plain;q=0.8,*/*;q=0.5".into(),
            max_bytes: 8 * 1024 * 1024,
        }
    }

    pub fn subresource(&self, top: &Url) -> Self {
        let mut ctx = self.clone();
        ctx.top_site = Some(top.clone());
        ctx.max_bytes = 4 * 1024 * 1024;
        ctx
    }
}

#[derive(Debug)]
pub struct Fetched {
    pub url: Url,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub truncated: bool,
    /// Number of cookies withheld because the request was third-party.
    pub cookies_withheld: u64,
}

#[derive(Debug)]
pub enum FetchOutcome {
    Ok(Fetched),
    Blocked { host: String, category: Category },
    Error(String),
}

pub struct HttpClient {
    agent: ureq::Agent,
    pub filter: Arc<FilterEngine>,
    pub jar: Arc<CookieJar>,
    pub dns: Arc<DnsResolver>,
    pub stats: Arc<Mutex<ShieldStats>>,
    pub requests: AtomicU64,
    pub cleaned: AtomicU64,
}

impl HttpClient {
    pub fn new(filter: FilterEngine) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(25))
            .timeout_write(Duration::from_secs(10))
            .redirects(0)
            .build();
        Self {
            agent,
            filter: Arc::new(filter),
            jar: Arc::new(CookieJar::new()),
            dns: Arc::new(DnsResolver::system()),
            stats: Arc::new(Mutex::new(ShieldStats::default())),
            requests: AtomicU64::new(0),
            cleaned: AtomicU64::new(0),
        }
    }

    pub fn stats_snapshot(&self) -> ShieldStats {
        self.stats.lock().unwrap().clone()
    }

    pub fn block(&self, host: &str, category: Category) {
        self.stats.lock().unwrap().record(category, host);
    }

    pub fn fetch(&self, url: &Url, ctx: &RequestContext) -> FetchOutcome {
        let mut current = url.clone();
        let mut redirects = 0;
        let mut cleaned_total = 0;

        loop {
            if ctx.shield.clean_urls {
                let (clean, removed) = urlclean::clean(&current);
                if !removed.is_empty() {
                    cleaned_total += removed.len();
                    current = clean;
                }
            }
            if let Some(host) = current.host_str() {
                let top_host = ctx
                    .top_site
                    .as_ref()
                    .and_then(|u| u.host_str())
                    .map(|h| h.to_ascii_lowercase());
                let host_lower = host.to_ascii_lowercase();

                // CNAME cloaking: a first-party looking subdomain that really
                // points at a tracker.
                if ctx.shield.cname_cloak {
                    let first_party_subdomain = top_host
                        .as_ref()
                        .map(|top| {
                            !is_third_party(&host_lower, top)
                                && registrable_domain(&host_lower)
                                    != registrable_domain(top)
                        })
                        .unwrap_or(false);
                    if first_party_subdomain && self.dns.is_available() {
                        let chain = self.dns.cname_chain(&host_lower);
                        if chain
                            .iter()
                            .any(|target| self.filter.is_cloak_target(target).is_some())
                        {
                            self.block(&host_lower, Category::CnameTracker);
                            return FetchOutcome::Blocked {
                                host: host_lower,
                                category: Category::CnameTracker,
                            };
                        }
                    }
                }

                let decision = self.filter.check(&host_lower, top_host.as_deref(), &ctx.shield);
                if decision.blocked {
                    let category = decision.category.unwrap_or(Category::Tracker);
                    self.block(&host_lower, category);
                    return FetchOutcome::Blocked {
                        host: host_lower,
                        category,
                    };
                }
            } else {
                return FetchOutcome::Error("URL без хоста".into());
            }

            self.requests.fetch_add(1, Ordering::Relaxed);
            let referer = referrer_value(&current, ctx);
            let cookie_header = self.jar.header_for(
                &ctx.container,
                &current,
                ctx.top_site.as_ref().and_then(|u| u.host_str()),
                !ctx.shield.block_third_party_cookies,
                crate::util::now_ts(),
            );

            let ua = if ctx.shield.uniform_client { UNIFORM_UA } else { AEGIS_UA };
            let mut req = self
                .agent
                .get(current.as_str())
                .set("User-Agent", ua)
                .set("Accept", &ctx.accept)
                .set("Accept-Language", "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7")
                .set("DNT", "1")
                .set("Sec-GPC", "1")
                .set("Connection", "close");
            if let Some(r) = referer {
                req = req.set("Referer", &r);
            }
            if let Some(c) = cookie_header {
                req = req.set("Cookie", &c);
            }

            let response = match req.call() {
                Ok(r) => r,
                Err(ureq::Error::Status(code, r)) => {
                    if code >= 300 && code < 400 {
                        r
                    } else {
                        let text = format!("HTTP {code}");
                        return FetchOutcome::Error(text);
                    }
                }
                Err(ureq::Error::Transport(t)) => {
                    return FetchOutcome::Error(friendly_transport_error(&t.to_string()))
                }
            };

            let status = response.status();
            let content_type = response
                .header("content-type")
                .unwrap_or("application/octet-stream")
                .to_string();
            let set_cookies: Vec<String> = response
                .all("set-cookie")
                .iter()
                .map(|s| s.to_string())
                .collect();
            if !set_cookies.is_empty() {
                self.jar
                    .store_from_response(&ctx.container, &current, &set_cookies, crate::util::now_ts());
            }

            if (300..400).contains(&status) {
                if let Some(location) = response.header("location") {
                    redirects += 1;
                    if redirects > 8 {
                        return FetchOutcome::Error("Слишком много перенаправлений".into());
                    }
                    let next = current.join(location).ok();
                    match next {
                        Some(u) if matches!(u.scheme(), "http" | "https") => {
                            current = u;
                            continue;
                        }
                        _ => return FetchOutcome::Error("Некорректное перенаправление".into()),
                    }
                }
                return FetchOutcome::Error("Перенаправление без адреса".into());
            }

            let mut body = Vec::new();
            let mut truncated = false;
            match response.into_reader().take(ctx.max_bytes as u64 + 1).read_to_end(&mut body) {
                Ok(_) => {
                    if body.len() > ctx.max_bytes {
                        body.truncate(ctx.max_bytes);
                        truncated = true;
                    }
                }
                Err(e) => return FetchOutcome::Error(format!("Ошибка чтения: {e}")),
            }

            if cleaned_total > 0 {
                self.cleaned.fetch_add(cleaned_total as u64, Ordering::Relaxed);
            }
            return FetchOutcome::Ok(Fetched {
                url: current,
                status,
                content_type,
                body,
                truncated,
                cookies_withheld: 0,
            });
        }
    }

    /// Convenience for small subresources with a byte cap.
    pub fn fetch_text(&self, url: &Url, ctx: &RequestContext) -> Option<String> {
        match self.fetch(url, ctx) {
            FetchOutcome::Ok(f) => Some(decode_body(&f.body, &f.content_type)),
            _ => None,
        }
    }

    pub fn fetch_bytes(&self, url: &Url, ctx: &RequestContext) -> Option<Vec<u8>> {
        match self.fetch(url, ctx) {
            FetchOutcome::Ok(f) => Some(f.body),
            _ => None,
        }
    }
}

fn referrer_value(target: &Url, ctx: &RequestContext) -> Option<String> {
    match ctx.shield.referrer {
        ReferrerPolicy::None => None,
        ReferrerPolicy::Normal => Some(target.to_string()),
        ReferrerPolicy::SameSiteOrigin => {
            let top = ctx.top_site.as_ref()?;
            let same = match (target.host_str(), top.host_str()) {
                (Some(a), Some(b)) => registrable_domain(a) == registrable_domain(b),
                _ => false,
            };
            if same {
                Some(format!(
                    "{}://{}",
                    target.scheme(),
                    target.host_str().unwrap_or("")
                ))
            } else {
                None
            }
        }
    }
}

fn friendly_transport_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("dns") || lower.contains("resolve") || lower.contains("name") {
        "Не удалось найти сайт (DNS)".into()
    } else if lower.contains("certificate") || lower.contains("tls") {
        "Проблема с TLS-сертификатом сайта".into()
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "Сайт не ответил вовремя".into()
    } else if lower.contains("connection refused") {
        "Соединение отклонено".into()
    } else {
        format!("Сетевая ошибка: {raw}")
    }
}

/// Decode a response body using the BOM, the `charset` parameter or the
/// `<meta charset>` tag inside the document.
pub fn decode_body(bytes: &[u8], content_type: &str) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).to_string();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        return text.to_string();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(bytes);
        return text.to_string();
    }
    let mut label = content_type
        .to_ascii_lowercase()
        .find("charset=")
        .map(|i| {
            let rest = &content_type[i + 8..];
            rest.split([';', ' ', '"']).next().unwrap_or("").to_string()
        });
    if label.is_none() {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
        if let Some(i) = head.find("charset=") {
            let rest = &head[i + 8..];
            let rest = rest.trim_start_matches([' ', '"', '\'']);
            let value: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !value.is_empty() {
                label = Some(value);
            }
        }
    }
    if let Some(label) = label {
        if let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) {
            let (text, _, _) = enc.decode(bytes);
            return text.to_string();
        }
    }
    String::from_utf8_lossy(bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_meta_charset() {
        let html = "<meta charset=\"windows-1251\"><p>Привет</p>";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(html.split("<p>").next().unwrap().as_bytes());
        let (encoded, _, _) = encoding_rs::WINDOWS_1251.encode("Привет");
        bytes.extend_from_slice(b"<p>");
        bytes.extend_from_slice(&encoded);
        let text = decode_body(&bytes, "text/html");
        assert!(text.contains("Привет"));
    }

    #[test]
    fn unknown_charset_falls_back() {
        let text = decode_body("hello".as_bytes(), "text/plain; charset=nonsense");
        assert_eq!(text, "hello");
    }
}
