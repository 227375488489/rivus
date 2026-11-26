use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "Rivus";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "227375488489/rivus";

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SearchEngine {
    pub id: String,
    pub name: String,
    /// `%s` is replaced with the percent-encoded query.
    pub template: String,
    /// Firefox-style shortcut typed in the address bar: `@ddg rust`.
    pub keyword: String,
    /// Chip colour in the omnibox, e.g. `#DE5833`.
    pub color: String,
    pub builtin: bool,
}

impl SearchEngine {
    pub fn query_url(&self, query: &str) -> String {
        self.template.replace(
            "%s",
            &percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
                .to_string(),
        )
    }
}

pub fn builtin_engines() -> Vec<SearchEngine> {
    let e = |id: &str, name: &str, template: &str, keyword: &str, color: &str| SearchEngine {
        id: id.to_string(),
        name: name.to_string(),
        template: template.to_string(),
        keyword: keyword.to_string(),
        color: color.to_string(),
        builtin: true,
    };
    vec![
        e("ddg", "DuckDuckGo", "https://html.duckduckgo.com/html/?q=%s", "ddg", "#DE5833"),
        e("startpage", "Startpage", "https://www.startpage.com/sp/search?query=%s", "sp", "#5C7CFA"),
        e("brave", "Brave Search", "https://search.brave.com/search?q=%s", "brave", "#FB542B"),
        e("google", "Google", "https://www.google.com/search?q=%s&gbv=1", "g", "#4285F4"),
        e("bing", "Bing", "https://www.bing.com/search?q=%s", "b", "#0AA1DD"),
        e("yandex", "Yandex", "https://yandex.ru/search/?text=%s", "ya", "#FC3F1D"),
        e("ecosia", "Ecosia", "https://www.ecosia.org/search?q=%s", "eco", "#2E8B57"),
        e("qwant", "Qwant", "https://www.qwant.com/?q=%s", "qw", "#5C97FF"),
        e("mojeek", "Mojeek", "https://www.mojeek.com/search?q=%s", "mj", "#1E7B4F"),
        e("marginalia", "Marginalia", "https://search.marginalia.nu/search?query=%s", "marg", "#A855F7"),
        e("wikipedia", "Wikipedia", "https://ru.wikipedia.org/w/index.php?search=%s", "w", "#94A3B8"),
        e("searx", "SearXNG (свой инстанс)", "https://searx.be/search?q=%s", "sx", "#38BDF8"),
    ]
}

// ---------------------------------------------------------------------------
// Shield
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferrerPolicy {
    /// Send no Referer at all (default).
    None,
    /// Send only the origin, and only within the same site.
    SameSiteOrigin,
    /// Browser default behaviour.
    Normal,
}

impl Default for ReferrerPolicy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Shield {
    pub ads: bool,
    pub trackers: bool,
    pub miners: bool,
    /// Resolve CNAME chains of first-party subdomains and block hidden trackers.
    pub cname_cloak: bool,
    /// Strip utm_*, fbclid, gclid … from every navigation and from copied links.
    pub clean_urls: bool,
    /// Never send a cookie to a site other than the one you are visiting.
    pub block_third_party_cookies: bool,
    /// Send one generic User-Agent + Client Hints for everyone.
    pub uniform_client: bool,
    pub referrer: ReferrerPolicy,
    /// Do not allow the page to trigger extra requests for prefetch/preconnect.
    pub block_prefetch: bool,
}

impl Default for Shield {
    fn default() -> Self {
        Self {
            ads: true,
            trackers: true,
            miners: true,
            cname_cloak: true,
            clean_urls: true,
            block_third_party_cookies: true,
            uniform_client: true,
            referrer: ReferrerPolicy::None,
            block_prefetch: true,
        }
    }
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShieldStats {
    pub ads: u64,
    pub trackers: u64,
    pub miners: u64,
    pub cname: u64,
    pub cookies: u64,
    pub cleaned_urls: u64,
    pub blocked_hosts: std::collections::BTreeMap<String, u64>,
}

impl ShieldStats {
    pub fn total(&self) -> u64 {
        self.ads + self.trackers + self.miners + self.cname + self.cookies
    }

    pub fn record(&mut self, category: crate::filter::Category, host: &str) {
        match category {
            crate::filter::Category::Ads => self.ads += 1,
            crate::filter::Category::Tracker => self.trackers += 1,
            crate::filter::Category::Miner => self.miners += 1,
            crate::filter::Category::CnameTracker => self.cname += 1,
        }
        *self.blocked_hosts.entry(host.to_string()).or_insert(0) += 1;
    }
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub color: String,
    pub icon: String,
    /// Storage, cookies and history are wiped when the last tab closes.
    pub ephemeral: bool,
    pub created_at: i64,
}

impl Container {
    pub fn personal() -> Self {
        Self {
            id: "personal".into(),
            name: "Личное".into(),
            color: "#5B9DFF".into(),
            icon: "shield".into(),
            ephemeral: false,
            created_at: crate::util::now_ts(),
        }
    }
}

// ---------------------------------------------------------------------------
// History / bookmarks / session
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    pub ts: i64,
    pub url: String,
    pub title: String,
    pub container: String,
    #[serde(default)]
    pub visits: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub container: String,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TabSession {
    pub url: String,
    pub title: String,
    pub container: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PasswordEntry {
    pub id: String,
    pub site: String,
    pub username: String,
    pub password: String,
    pub note: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClearPolicy {
    Never,
    OnExit,
    /// Only ephemeral containers are wiped (always true regardless of this).
    EphemeralOnly,
}

impl Default for ClearPolicy {
    fn default() -> Self {
        Self::EphemeralOnly
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "url")]
pub enum Homepage {
    NewTab,
    Custom(String),
}

impl Default for Homepage {
    fn default() -> Self {
        Self::NewTab
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub locale: String,
    pub theme: String,
    pub accent: String,
    pub density: String,
    pub ui_scale: f32,
    pub page_zoom: f32,
    /// Invisible border around the page, in logical pixels (letterboxing).
    pub letterbox: u8,
    pub default_engine: String,
    pub engines: Vec<SearchEngine>,
    pub shield: Shield,
    pub homepage: Homepage,
    pub restore_session: bool,
    pub clear_on_exit: ClearPolicy,
    /// Sites with a per-site shield override.
    pub shield_overrides: Vec<SiteRule>,
    pub custom_css: String,
    pub font_page: Option<String>,
    pub font_ui: Option<String>,
    pub show_bookmarks_bar: bool,
    pub containers: Vec<Container>,
    pub auto_update: bool,
    pub update_channel: String,
    pub last_update_check: i64,
    pub skipped_version: String,
    pub search_in_page: bool,
    pub downloads_dir: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            locale: "ru".into(),
            theme: "aegis".into(),
            accent: "#5B9DFF".into(),
            density: "cozy".into(),
            ui_scale: 1.0,
            page_zoom: 1.0,
            letterbox: 4,
            default_engine: "ddg".into(),
            engines: builtin_engines(),
            shield: Shield::default(),
            homepage: Homepage::NewTab,
            restore_session: true,
            clear_on_exit: ClearPolicy::EphemeralOnly,
            shield_overrides: Vec::new(),
            custom_css: String::new(),
            font_page: None,
            font_ui: None,
            show_bookmarks_bar: false,
            containers: vec![Container::personal()],
            auto_update: true,
            update_channel: "stable".into(),
            last_update_check: 0,
            skipped_version: String::new(),
            search_in_page: true,
            downloads_dir: None,
        }
    }
}

impl Settings {
    pub fn engine(&self, id: &str) -> SearchEngine {
        self.engines
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .unwrap_or_else(|| builtin_engines()[0].clone())
    }

    pub fn default_engine(&self) -> SearchEngine {
        self.engine(&self.default_engine)
    }

    pub fn container(&self, id: &str) -> Option<&Container> {
        self.containers.iter().find(|c| c.id == id)
    }

    pub fn shield_for(&self, host: &str) -> Shield {
        if let Some(rule) = self
            .shield_overrides
            .iter()
            .find(|r| crate::filter::domain_matches(host, &r.host))
        {
            if !rule.enabled {
                return Shield {
                    ads: false,
                    trackers: false,
                    miners: false,
                    cname_cloak: false,
                    clean_urls: self.shield.clean_urls,
                    block_third_party_cookies: false,
                    uniform_client: self.shield.uniform_client,
                    referrer: self.shield.referrer,
                    block_prefetch: false,
                };
            }
        }
        self.shield.clone()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SiteRule {
    pub host: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub published_at: String,
    pub asset_name: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub checksums_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct UpdateState {
    pub checked_at: i64,
    pub available: Option<ReleaseInfo>,
    pub last_error: Option<String>,
    pub downloaded_path: Option<String>,
}
