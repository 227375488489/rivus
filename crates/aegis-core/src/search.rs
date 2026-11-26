//! Turning whatever is typed in the address bar into a URL or a search.
//!
//! The address bar never asks a server for suggestions. Everything below is
//! local: history, bookmarks and the engine registry.

use crate::models::{SearchEngine, Settings};
use url::Url;

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Url(Url),
    Search { engine: SearchEngine, query: String },
}

impl Target {
    pub fn display(&self) -> String {
        match self {
            Target::Url(u) => u.to_string(),
            Target::Search { engine, query } => format!("{}: {query}", engine.name),
        }
    }
}

/// `@ddg rust` / `!ddg rust` select an engine for a single search.
pub fn parse_with_engines(input: &str, engines: &[SearchEngine], default: &SearchEngine) -> Target {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix('@').or_else(|| trimmed.strip_prefix('!')) {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("").trim();
        let query = parts.next().unwrap_or("").trim();
        if !query.is_empty() {
            if let Some(engine) = engines.iter().find(|e| e.keyword.eq_ignore_ascii_case(key)) {
                return Target::Search {
                    engine: engine.clone(),
                    query: query.to_string(),
                };
            }
        }
    }

    if let Some(url) = as_url(trimmed) {
        return Target::Url(url);
    }
    Target::Search {
        engine: default.clone(),
        query: trimmed.to_string(),
    }
}

pub fn parse(input: &str, settings: &Settings) -> Target {
    parse_with_engines(input, &settings.engines, &settings.default_engine())
}

pub fn as_url(input: &str) -> Option<Url> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    // Explicit scheme.
    if let Ok(url) = Url::parse(t) {
        match url.scheme() {
            "http" | "https" | "aegis" => return Some(url),
            _ => {}
        }
        if t.contains("://") {
            return None;
        }
    }
    if t.starts_with("about:") || t.starts_with("view-source:") {
        return None;
    }
    // localhost / host:port / bare IPv4 / domain with a dot and no spaces.
    if t.contains(char::is_whitespace) {
        return None;
    }
    let host_candidate = t.split(['/', '?', '#']).next().unwrap_or(t);
    let has_port = host_candidate
        .rsplit_once(':')
        .map(|(_, p)| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
        .unwrap_or(false);
    let bare = if has_port {
        host_candidate.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_candidate)
    } else {
        host_candidate
    };
    let looks_host = bare == "localhost"
        || bare.parse::<std::net::IpAddr>().is_ok()
        || (bare.contains('.') && !bare.starts_with('.') && !bare.ends_with('.'));
    if !looks_host {
        return None;
    }
    Url::parse(&format!("http://{t}")).ok()
}

/// Local suggestions: history + bookmarks + open tabs, never the network.
pub fn suggestions(
    input: &str,
    history: &[crate::models::HistoryEntry],
    bookmarks: &[crate::models::Bookmark],
    extra: &[String],
    limit: usize,
) -> Vec<String> {
    let q = input.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(i32, String)> = Vec::new();
    let mut push = |score: i32, value: String| {
        if !out.iter().any(|(_, v)| v.eq_ignore_ascii_case(&value)) {
            out.push((score, value));
        }
    };
    for b in bookmarks {
        let hay = format!("{} {}", b.title, b.url).to_lowercase();
        if hay.contains(&q) {
            push(100 + (b.title.to_lowercase().starts_with(&q) as i32) * 10, b.url.clone());
        }
    }
    for h in history {
        let hay = format!("{} {}", h.title, h.url).to_lowercase();
        if hay.contains(&q) {
            let starts = h.url.to_lowercase().starts_with(&q)
                || h.title.to_lowercase().starts_with(&q);
            push(60 + starts as i32 * 10 + (h.visits.min(20) as i32), h.url.clone());
        }
    }
    for e in extra {
        if e.to_lowercase().contains(&q) {
            push(20, e.clone());
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, v)| v).take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{builtin_engines, Settings};

    #[test]
    fn url_vs_search() {
        let s = Settings::default();
        assert!(matches!(parse("example.com", &s), Target::Url(_)));
        assert!(matches!(parse("https://a.b/c", &s), Target::Url(_)));
        assert!(matches!(parse("localhost:8080", &s), Target::Url(_)));
        assert!(matches!(parse("how to rust", &s), Target::Search { .. }));
        assert!(matches!(parse("привет", &s), Target::Search { .. }));
        assert!(matches!(parse("1.1.1.1", &s), Target::Url(_)));
    }

    #[test]
    fn keyword_selects_engine() {
        let engines = builtin_engines();
        let default = engines[0].clone();
        match parse_with_engines("@g rust", &engines, &default) {
            Target::Search { engine, query } => {
                assert_eq!(engine.id, "google");
                assert_eq!(query, "rust");
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn escaping_is_safe() {
        let s = Settings::default();
        match parse("a&b=c d", &s) {
            Target::Search { engine, query } => {
                let url = engine.query_url(&query);
                assert!(!url.contains(' '));
                assert!(url.contains("a%26b%3Dc%20d") || url.contains("a%26b%3Dc+d"));
            }
            _ => panic!("expected search"),
        }
    }
}
