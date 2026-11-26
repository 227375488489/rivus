//! Domain-level blocking, decided before a single packet is sent.
//!
//! Rules are plain hostnames in `hosts` or one-per-line format. Matching is
//! suffix based, so `doubleclick.net` also covers `ad.doubleclick.net`.

use crate::models::Shield;
use crate::util;
use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Ads,
    Tracker,
    Miner,
    CnameTracker,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Ads => "Реклама",
            Category::Tracker => "Трекер",
            Category::Miner => "Криптомайнер",
            Category::CnameTracker => "CNAME-трекер",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub blocked: bool,
    pub category: Option<Category>,
}

impl Decision {
    pub const ALLOW: Decision = Decision { blocked: false, category: None };
    pub fn block(category: Category) -> Self {
        Self { blocked: true, category: Some(category) }
    }
}

#[derive(Default, Clone, Debug)]
pub struct DomainSet {
    set: HashSet<String>,
}

impl DomainSet {
    pub fn from_text(text: &str) -> Self {
        let mut set = HashSet::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            // hosts format: "0.0.0.0 tracker.example"
            let host = if line.contains(char::is_whitespace) {
                line.split_whitespace().last().unwrap_or("").to_string()
            } else {
                line.to_string()
            };
            let host = host.trim_matches('.').to_ascii_lowercase();
            if host.is_empty() || host == "0.0.0.0" || host == "localhost" {
                continue;
            }
            set.insert(host);
        }
        Self { set }
    }

    pub fn extend(&mut self, other: &DomainSet) {
        self.set.extend(other.set.iter().cloned());
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn contains(&self, host: &str) -> bool {
        let host = host.trim_matches('.').to_ascii_lowercase();
        if self.set.contains(&host) {
            return true;
        }
        let mut idx = 0;
        while let Some(pos) = host[idx..].find('.') {
            idx += pos + 1;
            if self.set.contains(&host[idx..]) {
                return true;
            }
            if idx >= host.len() {
                break;
            }
        }
        false
    }
}

#[derive(Clone, Debug, Default)]
pub struct FilterEngine {
    pub ads: DomainSet,
    pub trackers: DomainSet,
    pub miners: DomainSet,
    pub cloak: DomainSet,
    pub user: DomainSet,
}

impl FilterEngine {
    pub fn builtin() -> Self {
        Self {
            ads: DomainSet::from_text(include_str!("../data/ads.txt")),
            trackers: DomainSet::from_text(include_str!("../data/trackers.txt")),
            miners: DomainSet::from_text(include_str!("../data/miners.txt")),
            cloak: DomainSet::from_text(include_str!("../data/cloak.txt")),
            user: DomainSet::default(),
        }
    }

    /// Load every `*.txt` in `<data>/filters` and merge it into the user set.
    pub fn load_user_lists(&mut self, dir: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let set = DomainSet::from_text(&text);
                    count += set.len();
                    self.user.extend(&set);
                }
            }
        }
        count
    }

    pub fn from_data_dir() -> Self {
        let mut engine = Self::builtin();
        engine.load_user_lists(&util::data_dir().join("filters"));
        engine
    }

    /// `top_site` is the registrable domain the user is actually visiting.
    pub fn check(&self, host: &str, top_site: Option<&str>, shield: &Shield) -> Decision {
        let host = host.trim_matches('.').to_ascii_lowercase();
        if host.is_empty() || host == "localhost" || host.parse::<std::net::IpAddr>().is_ok() {
            return Decision::ALLOW;
        }
        if self.user.contains(&host) {
            return Decision::block(Category::Tracker);
        }
        if shield.miners && self.miners.contains(&host) {
            return Decision::block(Category::Miner);
        }
        if shield.ads && self.ads.contains(&host) {
            return Decision::block(Category::Ads);
        }
        if shield.trackers && self.trackers.contains(&host) {
            return Decision::block(Category::Tracker);
        }
        // A first-party host is never blocked: the page you asked for must load.
        let _ = top_site;
        Decision::ALLOW
    }

    pub fn is_cloak_target(&self, host: &str) -> Option<Category> {
        if self.cloak.contains(host) || self.trackers.contains(host) {
            Some(Category::CnameTracker)
        } else {
            None
        }
    }
}

pub fn domain_matches(host: &str, rule: &str) -> bool {
    let host = host.trim_matches('.').to_ascii_lowercase();
    let rule = rule.trim_matches('.').to_ascii_lowercase();
    host == rule || host.ends_with(&format!(".{rule}"))
}

pub fn is_third_party(host: &str, top_site: &str) -> bool {
    match (registrable_domain(host), registrable_domain(top_site)) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Public suffix handling, small embedded list
// ---------------------------------------------------------------------------

static PSL: &[&str] = &[
    "co.uk", "org.uk", "ac.uk", "gov.uk", "me.uk", "ltd.uk", "plc.uk", "net.uk", "sch.uk",
    "com.ru", "net.ru", "org.ru", "msk.ru", "spb.ru", "co.ru", "ac.ru", "gov.ru",
    "com.ua", "net.ua", "org.ua", "kiev.ua", "co.ua", "in.ua",
    "com.by", "net.by", "org.by", "gov.by", "minsk.by",
    "com.kz", "net.kz", "org.kz", "gov.kz",
    "com.tr", "net.tr", "org.tr", "gov.tr", "edu.tr",
    "com.cn", "net.cn", "org.cn", "gov.cn", "edu.cn", "com.hk", "org.hk", "com.tw", "org.tw",
    "co.jp", "ne.jp", "or.jp", "ac.jp", "go.jp", "com.sg", "edu.sg", "com.my", "com.ph",
    "com.id", "co.id", "or.id", "ac.id", "go.id", "com.vn", "com.th", "co.th", "in.th",
    "com.pk", "com.bd", "com.np", "com.lk", "co.in", "net.in", "org.in", "gen.in", "firm.in",
    "co.il", "org.il", "ac.il", "gov.il", "com.sa", "com.ae", "co.ae", "com.eg", "com.qa",
    "com.au", "net.au", "org.au", "edu.au", "gov.au", "co.nz", "net.nz", "org.nz", "govt.nz",
    "co.za", "org.za", "net.za", "gov.za", "com.br", "net.br", "org.br", "gov.br", "com.ar",
    "com.mx", "com.co", "com.pe", "com.ve", "com.ec", "com.uy", "com.py", "com.bo", "com.cl",
    "co.kr", "or.kr", "ne.kr", "go.kr", "ac.kr", "com.es", "org.es", "nom.es", "gob.es",
    "co.it", "gov.it", "com.pt", "org.pt", "gov.pt", "com.gr", "org.gr", "net.gr", "com.pl",
    "net.pl", "org.pl", "gov.pl", "com.cz", "co.at", "or.at", "gv.at", "com.ro", "com.hr",
    "com.ua", "co.hu", "com.bg", "com.md", "com.ge", "com.am", "com.az", "com.uz",
    "eu.org", "github.io", "gitlab.io", "pages.dev", "workers.dev", "vercel.app",
    "netlify.app", "herokuapp.com", "appspot.com", "blogspot.com", "wordpress.com",
    "tumblr.com", "medium.com", "substack.com", "notion.site", "fly.dev", "onrender.com",
    "s3.amazonaws.com", "cloudfront.net", "azurewebsites.net", "web.app", "firebaseapp.com",
];

pub fn registrable_domain(host: &str) -> Option<String> {
    let host = host.trim_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Some(host);
    }
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return Some(labels.join("."));
    }
    for i in 0..labels.len() {
        let suffix = labels[i..].join(".");
        if PSL.iter().any(|p| *p == suffix) {
            if i == 0 {
                return Some(suffix);
            }
            return Some(labels[i - 1..].join("."));
        }
    }
    Some(labels[labels.len() - 2..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffix_matching() {
        let set = DomainSet::from_text("doubleclick.net\n0.0.0.0 ads.example.org\n# comment\n");
        assert!(set.contains("doubleclick.net"));
        assert!(set.contains("ad.doubleclick.net"));
        assert!(set.contains("x.ads.example.org"));
        assert!(!set.contains("notdoubleclick.net"));
        assert!(!set.contains("example.org"));
    }

    #[test]
    fn etld_plus_one() {
        assert_eq!(registrable_domain("a.b.example.co.uk").as_deref(), Some("example.co.uk"));
        assert_eq!(registrable_domain("news.ycombinator.com").as_deref(), Some("ycombinator.com"));
        assert_eq!(registrable_domain("localhost").as_deref(), Some("localhost"));
        assert_eq!(registrable_domain("user.github.io").as_deref(), Some("user.github.io"));
    }

    #[test]
    fn third_party() {
        assert!(!is_third_party("cdn.example.com", "example.com"));
        assert!(is_third_party("tracker.net", "example.com"));
    }

    #[test]
    fn blocks_builtin() {
        let f = FilterEngine::builtin();
        let s = Shield::default();
        assert_eq!(f.check("ad.doubleclick.net", Some("example.com"), &s).category, Some(Category::Ads));
        assert!(f.check("example.com", Some("example.com"), &s).blocked == false);
        assert!(f.check("localhost", None, &s).blocked == false);
    }
}
