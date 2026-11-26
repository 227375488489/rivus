//! Opt-in updates from GitHub Releases.
//!
//! This is the only network request Rivus makes to its own infrastructure. It
//! runs only when the user asks, or once a day when `auto_update` is enabled.
//! The executable is verified against the release's SHA256SUMS asset before it
//! is allowed to replace the running binary.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPO: &str = "227375488489/rivus";
const MAX_DOWNLOAD: u64 = 220 * 1024 * 1024;
const UA: &str = concat!("Rivus/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub url: String,
    pub notes: String,
    pub published_at: String,
    pub asset: String,
    pub size: u64,
    pub download_url: String,
    pub checksums_url: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(20))
        .user_agent(UA)
        .build()
}

fn fetch_release() -> Result<Release, String> {
    let response = agent()
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Не удалось связаться с GitHub: {e}"))?;
    response
        .into_json::<Release>()
        .map_err(|e| format!("Некорректный ответ GitHub: {e}"))
}

pub fn check() -> Result<UpdateInfo, String> {
    let release = fetch_release()?;
    let latest = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("rivus.exe"))
        .ok_or_else(|| "В релизе нет файла rivus.exe".to_string())?;
    let checksums_url = release
        .assets
        .iter()
        .find(|a| {
            let n = a.name.to_ascii_lowercase();
            n.contains("sha256") || n.contains("checksums")
        })
        .map(|a| a.browser_download_url.clone());

    Ok(UpdateInfo {
        current: env!("CARGO_PKG_VERSION").to_string(),
        newer: version_cmp(&latest, env!("CARGO_PKG_VERSION")).is_gt(),
        latest,
        url: release.html_url,
        notes: release.body.unwrap_or_default(),
        published_at: release.published_at.unwrap_or_default(),
        asset: asset.name.clone(),
        size: asset.size,
        download_url: asset.browser_download_url.clone(),
        checksums_url,
    })
}

pub fn download(info: &UpdateInfo, directory: &Path) -> Result<PathBuf, String> {
    if !info.newer {
        return Err("Установлена актуальная версия".into());
    }
    let checksums_url = info
        .checksums_url
        .as_ref()
        .ok_or_else(|| "Релиз не содержит SHA256SUMS — установка отменена".to_string())?;
    let expected = fetch_checksum(checksums_url, &info.asset)?;

    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let target = directory.join("rivus-new.exe");
    let temp = directory.join("rivus-new.exe.part");
    let response = agent()
        .get(&info.download_url)
        .call()
        .map_err(|e| format!("Не удалось скачать обновление: {e}"))?;
    if let Some(length) = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
    {
        if length > MAX_DOWNLOAD {
            return Err("Файл обновления слишком большой".into());
        }
    }

    let mut hasher = Sha256::new();
    let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
    let mut reader = response.into_reader().take(MAX_DOWNLOAD + 1);
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_DOWNLOAD {
            let _ = fs::remove_file(&temp);
            return Err("Файл обновления слишком большой".into());
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    let actual = hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&temp);
        return Err("Контрольная сумма не совпала — обновление отклонено".into());
    }
    if fs::rename(&temp, &target).is_err() {
        fs::copy(&temp, &target).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&temp);
    }
    Ok(target)
}

fn fetch_checksum(url: &str, asset: &str) -> Result<String, String> {
    let body = agent()
        .get(url)
        .call()
        .map_err(|e| format!("Не удалось скачать SHA256SUMS: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    for line in body.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let name = name.trim_start_matches('*');
        if name.eq_ignore_ascii_case(asset) && hash.len() == 64 {
            return Ok(hash.to_string());
        }
    }
    Err("В SHA256SUMS нет файла rivus.exe".into())
}

pub fn apply(downloaded: &Path) -> Result<(), String> {
    if !downloaded.exists() {
        return Err("Сначала скачайте обновление".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = std::env::temp_dir().join(format!("rivus-update-{}.cmd", std::process::id()));
    let body = format!(
        "@echo off\r\n\
         setlocal\r\n\
         :wait\r\n\
         tasklist /FI \"PID eq {pid}\" 2>nul | find \"{pid}\" >nul\r\n\
         if not errorlevel 1 (\r\n\
           timeout /t 1 /nobreak >nul\r\n\
           goto wait\r\n\
         )\r\n\
         move /Y \"{new}\" \"{exe}\" >nul\r\n\
         start \"\" \"{exe}\"\r\n\
         del /f /q \"%~f0\" >nul 2>&1\r\n",
        pid = std::process::id(),
        new = downloaded.display(),
        exe = exe.display()
    );
    fs::write(&script, body).map_err(|e| e.to_string())?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        let _ = script;
        return Err("Автообновление поддерживается только в Windows-сборке".into());
    }
    Ok(())
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn parts(v: &str) -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }
    let a = parts(a);
    let b = parts(b);
    for i in 0..a.len().max(b.len()) {
        match a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}
