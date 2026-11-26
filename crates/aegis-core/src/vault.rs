//! Local password vault.
//!
//! Entries are sealed with ChaCha20-Poly1305. The 32-byte key is kept in the
//! Windows Credential Manager (keyring); if that is unavailable a key file
//! inside the data directory is used and the UI warns about it.
//! Nothing is ever uploaded.

use crate::models::{PasswordEntry, Shield};
use crate::util;
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SERVICE: &str = "aegis-browser";
const ACCOUNT: &str = "vault-master-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    CredentialManager,
    LocalFile,
}

impl KeySource {
    pub fn label(self) -> &'static str {
        match self {
            KeySource::CredentialManager => "Windows Credential Manager",
            KeySource::LocalFile => "локальный файл ключа",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    v: u32,
    nonce: String,
    data: String,
}

pub struct Vault {
    entries: Vec<PasswordEntry>,
    key: [u8; 32],
    path: PathBuf,
    pub key_source: KeySource,
}

impl Vault {
    pub fn open(root: &std::path::Path) -> Self {
        let (key, source) = load_or_create_key(root);
        let path = root.join("vault.bin");
        let mut vault = Self {
            entries: Vec::new(),
            key,
            path,
            key_source: source,
        };
        vault.load();
        vault
    }

    pub fn entries(&self) -> &[PasswordEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn add(&mut self, site: &str, username: &str, password: &str) -> String {
        let id = format!("p{}", rand::random::<u64>());
        let now = util::now_ts();
        self.entries.push(PasswordEntry {
            id: id.clone(),
            site: site.trim().to_string(),
            username: username.to_string(),
            password: password.to_string(),
            note: String::new(),
            created_at: now,
            updated_at: now,
        });
        let _ = self.save();
        id
    }

    pub fn update(&mut self, id: &str, username: &str, password: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.username = username.to_string();
            e.password = password.to_string();
            e.updated_at = util::now_ts();
            let _ = self.save();
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
        let _ = self.save();
    }

    pub fn for_host(&self, host: &str) -> Vec<&PasswordEntry> {
        self.entries
            .iter()
            .filter(|e| {
                let site = e
                    .site
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or("");
                !site.is_empty()
                    && (host.eq_ignore_ascii_case(site)
                        || host.ends_with(&format!(".{}", site.to_ascii_lowercase())))
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        let _ = self.save();
    }

    fn load(&mut self) {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        let Ok(sealed) = serde_json::from_slice::<Sealed>(&bytes) else {
            return;
        };
        let Ok(nonce) = STANDARD.decode(&sealed.nonce) else {
            return;
        };
        let Ok(data) = STANDARD.decode(&sealed.data) else {
            return;
        };
        if nonce.len() != 12 {
            return;
        }
        let Ok(cipher) = ChaCha20Poly1305::new_from_slice(&self.key) else {
            return;
        };
        if let Ok(plain) = cipher.decrypt(Nonce::from_slice(&nonce), data.as_ref()) {
            if let Ok(entries) = serde_json::from_slice::<Vec<PasswordEntry>>(&plain) {
                self.entries = entries;
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.key)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "key"))?;
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let plain = serde_json::to_vec(&self.entries)?;
        let data = cipher
            .encrypt(Nonce::from_slice(&nonce), plain.as_ref())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "encrypt"))?;
        let sealed = Sealed {
            v: 1,
            nonce: STANDARD.encode(nonce),
            data: STANDARD.encode(data),
        };
        util::write_json(&self.path, &sealed)
    }

    /// Used by the settings page to state honestly what is stored locally.
    pub fn summary(&self, shield: &Shield) -> String {
        format!(
            "{} записей · ключ: {} · третьи стороны: {}",
            self.entries.len(),
            self.key_source.label(),
            if shield.block_third_party_cookies { "заблокированы" } else { "разрешены" }
        )
    }
}

fn load_or_create_key(root: &std::path::Path) -> ([u8; 32], KeySource) {
    if let Ok(raw) = keyring_get() {
        if let Ok(bytes) = STANDARD.decode(raw) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return (key, KeySource::CredentialManager);
            }
        }
    }
    let key_path = root.join("vault.key");
    if let Ok(bytes) = std::fs::read(&key_path) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return (key, KeySource::LocalFile);
        }
    }
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    if keyring_set(&STANDARD.encode(key)).is_ok() {
        return (key, KeySource::CredentialManager);
    }
    let _ = std::fs::write(&key_path, key);
    (key, KeySource::LocalFile)
}

fn keyring_get() -> Result<String, keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)?.get_password()
}

fn keyring_set(value: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)?.set_password(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_encrypted() {
        let dir = std::env::temp_dir().join(format!("aegis-vault-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut vault = Vault::open(&dir);
        vault.add("example.com", "alice", "s3cret");
        assert_eq!(vault.count(), 1);
        let reopened = Vault::open(&dir);
        assert_eq!(reopened.count(), 1);
        assert_eq!(reopened.entries()[0].password, "s3cret");
        let raw = std::fs::read(dir.join("vault.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("s3cret"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
