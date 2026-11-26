//! All persistent state. Plain files under one directory, written atomically.
//!
//! Cookies live in `cookies.json`, settings in `settings.json`, history in an
//! append-only JSONL file. Deleting the directory is a complete reset.

use crate::models::{
    Bookmark, HistoryEntry, PasswordEntry, ReleaseInfo, Settings, TabSession, UpdateState,
};
use crate::util;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct Store {
    root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wipe {
    History,
    Bookmarks,
    Cookies,
    Passwords,
    Everything,
}

impl Store {
    pub fn open() -> std::io::Result<Self> {
        let root = util::data_dir();
        util::ensure_dir(&root)?;
        util::ensure_dir(&root.join("filters"))?;
        util::ensure_dir(&root.join("downloads"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn load_settings(&self) -> Settings {
        let mut settings: Settings = util::read_json(&self.path("settings.json")).unwrap_or_default();
        if settings.engines.is_empty() {
            settings.engines = crate::models::builtin_engines();
        }
        if settings.containers.is_empty() {
            settings.containers.push(crate::models::Container::personal());
        }
        settings
    }

    pub fn save_settings(&self, settings: &Settings) -> std::io::Result<()> {
        util::write_json(&self.path("settings.json"), settings)
    }

    // -- history ----------------------------------------------------------

    pub fn append_history(&self, entry: &HistoryEntry) -> std::io::Result<()> {
        util::ensure_dir(&self.root)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("history.jsonl"))?;
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Deduplicated, newest first. `limit` caps the result.
    pub fn history(&self, limit: usize) -> Vec<HistoryEntry> {
        let path = self.path("history.jsonl");
        let Ok(file) = fs::File::open(&path) else {
            return Vec::new();
        };
        let mut map: BTreeMap<String, HistoryEntry> = BTreeMap::new();
        for line in BufReader::new(file).lines().take(200_000).flatten() {
            if let Ok(mut entry) = serde_json::from_str::<HistoryEntry>(&line) {
                entry.visits = entry.visits.max(1);
                match map.get_mut(&entry.url) {
                    Some(existing) => {
                        existing.visits = existing.visits.saturating_add(entry.visits);
                        if entry.ts >= existing.ts {
                            existing.ts = entry.ts;
                            existing.title = entry.title.clone();
                        }
                        existing.container = entry.container;
                    }
                    None => {
                        map.insert(entry.url.clone(), entry);
                    }
                }
            }
        }
        let mut out: Vec<HistoryEntry> = map.into_values().collect();
        out.sort_by(|a, b| b.ts.cmp(&a.ts));
        out.truncate(limit);
        out
    }

    pub fn clear_history(&self) -> std::io::Result<()> {
        let path = self.path("history.jsonl");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    // -- bookmarks --------------------------------------------------------

    pub fn load_bookmarks(&self) -> Vec<Bookmark> {
        util::read_json(&self.path("bookmarks.json")).unwrap_or_default()
    }

    pub fn save_bookmarks(&self, bookmarks: &[Bookmark]) -> std::io::Result<()> {
        util::write_json(&self.path("bookmarks.json"), bookmarks)
    }

    // -- session ----------------------------------------------------------

    pub fn load_session(&self) -> Vec<TabSession> {
        util::read_json(&self.path("session.json")).unwrap_or_default()
    }

    pub fn save_session(&self, tabs: &[TabSession]) -> std::io::Result<()> {
        util::write_json(&self.path("session.json"), tabs)
    }

    // -- updates ----------------------------------------------------------

    pub fn load_update_state(&self) -> UpdateState {
        util::read_json(&self.path("update.json")).unwrap_or_default()
    }

    pub fn save_update_state(&self, state: &UpdateState) -> std::io::Result<()> {
        util::write_json(&self.path("update.json"), state)
    }

    pub fn save_release(&self, release: &ReleaseInfo) -> std::io::Result<()> {
        util::write_json(&self.path("pending-release.json"), release)
    }

    pub fn load_skipped(&self) -> String {
        self.load_settings().skipped_version
    }

    // -- passwords --------------------------------------------------------

    pub fn load_passwords(&self) -> Vec<PasswordEntry> {
        util::read_json(&self.path("passwords.json")).unwrap_or_default()
    }

    pub fn save_passwords(&self, entries: &[PasswordEntry]) -> std::io::Result<()> {
        util::write_json(&self.path("passwords.json"), entries)
    }

    // -- maintenance ------------------------------------------------------

    pub fn cookies_path(&self) -> PathBuf {
        self.path("cookies.json")
    }

    pub fn wipe(&self, what: Wipe) -> std::io::Result<()> {
        match what {
            Wipe::Everything => {
                if self.root.exists() {
                    fs::remove_dir_all(&self.root)?;
                }
                util::ensure_dir(&self.root)?;
                util::ensure_dir(&self.root.join("filters"))?;
                util::ensure_dir(&self.root.join("downloads"))?;
            }
            Wipe::History => self.clear_history()?,
            Wipe::Bookmarks => {
                let _ = fs::remove_file(self.path("bookmarks.json"));
            }
            Wipe::Cookies => {
                let _ = fs::remove_file(self.path("cookies.json"));
            }
            Wipe::Passwords => {
                let _ = fs::remove_file(self.path("passwords.json"));
            }
        }
        Ok(())
    }

    pub fn size_on_disk(&self) -> u64 {
        fn walk(dir: &Path) -> u64 {
            let mut total = 0;
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        total += walk(&path);
                    } else if let Ok(meta) = entry.metadata() {
                        total += meta.len();
                    }
                }
            }
            total
        }
        walk(&self.root)
    }

    pub fn open_data_folder(&self) {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("explorer").arg(&self.root).spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&self.root).spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_deduplicates() {
        let dir = std::env::temp_dir().join(format!("aegis-store-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("AEGIS_DATA_DIR", &dir);
        let store = Store::open().unwrap();
        store
            .append_history(&HistoryEntry {
                ts: 1,
                url: "https://a.example/".into(),
                title: "A".into(),
                container: "personal".into(),
                visits: 1,
            })
            .unwrap();
        store
            .append_history(&HistoryEntry {
                ts: 2,
                url: "https://a.example/".into(),
                title: "A2".into(),
                container: "personal".into(),
                visits: 1,
            })
            .unwrap();
        let history = store.history(10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].visits, 2);
        assert_eq!(history[0].title, "A2");
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("AEGIS_DATA_DIR");
    }
}
