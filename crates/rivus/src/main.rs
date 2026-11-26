#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use aegis_core::http::{decode_body, FetchOutcome, HttpClient, RequestContext};
use aegis_core::models::{
    Bookmark, Container, HistoryEntry, PasswordEntry, Settings, ShieldStats, TabSession,
};
use aegis_core::store::{Store, Wipe};
use aegis_core::urlclean;
use aegis_core::util;
use aegis_core::vault::Vault;
use aegis_engine::{FetchedResource, Fetcher, Page, RenderOptions, Renderer};
use base64::Engine;
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use url::Url;

mod proxy;
mod update;

const PAGE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

const PAGE_SCRIPT: &str = r#"
(() => {
  try {
    Object.defineProperty(navigator, 'doNotTrack', { get: () => '1', configurable: true });
    Object.defineProperty(navigator, 'globalPrivacyControl', { get: () => true, configurable: true });
  } catch (e) {}
  const hide = () => {
    try {
      const style = document.createElement('style');
      style.textContent = 'ins.adsbygoogle,[id^="google_ads"],.adsbygoogle,[class*="advert"],[id*="banner-ad"],iframe[src*="doubleclick"],[data-ad-slot]{display:none!important}';
      document.documentElement && document.documentElement.appendChild(style);
    } catch (e) {}
  };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', hide, { once: true });
  } else {
    hide();
  }

  // In-page find bar. It lives in a closed shadow root so site CSS cannot touch it.
  window.__rivusFind = () => {
    try {
      const existing = window.__rivusFindBox;
      if (existing) { existing.input.focus(); existing.input.select(); return; }
      const host = document.createElement('div');
      host.style.cssText = 'all:initial;position:fixed;z-index:2147483647;top:14px;right:18px;';
      const root = host.attachShadow({ mode: 'closed' });
      root.innerHTML =
        '<style>' +
        '*{box-sizing:border-box;font-family:system-ui,Segoe UI,sans-serif}' +
        '.box{display:flex;align-items:center;gap:10px;background:#0b1626;border:1px solid #2a4a70;' +
        'border-radius:12px;padding:8px 10px;box-shadow:0 18px 50px -12px #000;color:#eaf3ff}' +
        'input{all:unset;width:240px;font-size:13px;color:#eaf3ff}' +
        'input::placeholder{color:#7189a8}' +
        '.count{font-size:11px;color:#a9bdd8;min-width:76px;text-align:right}' +
        'button{all:unset;cursor:pointer;color:#a9bdd8;font-size:16px;line-height:1;padding:2px 4px;border-radius:6px}' +
        'button:hover{background:#183052;color:#fff}' +
        '</style>' +
        '<div class="box"><input placeholder="Найти на странице" /><span class="count"></span><button title="Закрыть">×</button></div>';
      document.documentElement.appendChild(host);
      const input = root.querySelector('input');
      const count = root.querySelector('.count');
      const close = root.querySelector('button');
      const search = (forward) => {
        const q = input.value;
        if (!q) { count.textContent = ''; return; }
        let found = false;
        try { found = window.find(q, false, !forward, true, false, true, false); } catch (e) {}
        count.textContent = found ? 'Найдено' : 'Нет совпадений';
      };
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); search(!e.shiftKey); }
        else if (e.key === 'Escape') { e.preventDefault(); host.remove(); window.__rivusFindBox = null; }
      });
      close.addEventListener('click', () => { host.remove(); window.__rivusFindBox = null; });
      window.__rivusFindBox = { input, host };
      input.focus();
    } catch (e) {}
  };
  window.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && (e.key === 'f' || e.key === 'F')) { e.preventDefault(); e.stopPropagation(); window.__rivusFind(); }
    else if (e.key === 'Escape' && window.__rivusFindBox) {
      window.__rivusFindBox.host.remove();
      window.__rivusFindBox = null;
    }
  }, true);
})();
"#;

struct PageView {
    webview: tauri::Webview,
    label: String,
    ephemeral: bool,
    directory: std::path::PathBuf,
}

struct Pages {
    views: HashMap<String, PageView>,
    bounds: (f64, f64, f64, f64),
    visible: Option<String>,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            views: HashMap::new(),
            bounds: (56.0, 110.0, 1200.0, 700.0),
            visible: None,
        }
    }
}

struct AppState {
    store: Arc<Store>,
    settings: Mutex<Settings>,
    shield: Arc<Mutex<aegis_core::models::Shield>>,
    client: Arc<HttpClient>,
    vault: Mutex<Vault>,
    renderer: Arc<Mutex<Renderer>>,
    pages: Mutex<Pages>,
    page_create: Mutex<()>,
    proxy_port: u16,
}

#[derive(Deserialize)]
struct NavigateReq {
    url: String,
    container: String,
    width: f32,
    zoom: f32,
    reader: bool,
}

#[derive(Serialize)]
struct LinkDto {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    url: String,
}

#[derive(Serialize)]
struct LineDto {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: String,
}

#[derive(Serialize)]
struct PageDto {
    url: String,
    title: String,
    png: String,
    width: u32,
    height: u32,
    css_width: f32,
    css_height: f32,
    links: Vec<LinkDto>,
    lines: Vec<LineDto>,
    text: String,
    js_heavy: bool,
    reader: bool,
    status: u16,
    truncated: bool,
    elapsed_ms: u128,
    blocked: ShieldStats,
    container: String,
}

#[derive(Serialize)]
struct Bootstrap {
    version: String,
    settings: Settings,
    history: Vec<HistoryEntry>,
    bookmarks: Vec<Bookmark>,
    containers: Vec<Container>,
    passwords: Vec<PasswordEntry>,
    stats: ShieldStats,
    data_dir: String,
    size_on_disk: u64,
    session: Vec<TabSession>,
}

struct Loader<'a> {
    client: &'a HttpClient,
    ctx: RequestContext,
    top: Url,
    cache: HashMap<String, Option<FetchedResource>>,
}

impl Loader<'_> {
    fn get(&mut self, url: &Url) -> Option<FetchedResource> {
        if let Some(hit) = self.cache.get(url.as_str()) {
            return hit.clone();
        }
        let ctx = self.ctx.subresource(&self.top);
        let out = match self.client.fetch(url, &ctx) {
            FetchOutcome::Ok(f) if f.status < 300 || f.status == 304 => Some(FetchedResource {
                bytes: f.body,
                content_type: Some(f.content_type),
            }),
            _ => None,
        };
        self.cache.insert(url.as_str().to_owned(), out.clone());
        out
    }
}

impl Fetcher for Loader<'_> {
    fn fetch(&mut self, url: &Url) -> Option<FetchedResource> {
        self.get(url)
    }
}

fn delta(before: &ShieldStats, after: &ShieldStats) -> ShieldStats {
    ShieldStats {
        ads: after.ads.saturating_sub(before.ads),
        trackers: after.trackers.saturating_sub(before.trackers),
        miners: after.miners.saturating_sub(before.miners),
        cname: after.cname.saturating_sub(before.cname),
        cookies: after.cookies.saturating_sub(before.cookies),
        cleaned_urls: after.cleaned_urls.saturating_sub(before.cleaned_urls),
        blocked_hosts: std::collections::BTreeMap::new(),
    }
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let settings = state.settings.lock().unwrap().clone();
    let stats = state.client.stats_snapshot();
    let passwords = state.vault.lock().unwrap().entries().to_vec();
    Ok(Bootstrap {
        version: env!("CARGO_PKG_VERSION").to_string(),
        containers: settings.containers.clone(),
        settings,
        history: state.store.history(500),
        bookmarks: state.store.load_bookmarks(),
        passwords,
        stats,
        data_dir: state.store.root().display().to_string(),
        size_on_disk: state.store.size_on_disk(),
        session: state.store.load_session(),
    })
}

#[tauri::command]
async fn navigate(state: State<'_, AppState>, req: NavigateReq) -> Result<PageDto, String> {
    let settings = state.settings.lock().unwrap().clone();
    let client = state.client.clone();
    let store = state.store.clone();
    let renderer = state.renderer.clone();
    tauri::async_runtime::spawn_blocking(move || render_page(&settings, &client, &store, &renderer, req))
        .await
        .map_err(|e| e.to_string())?
}

fn render_page(
    settings: &Settings,
    client: &HttpClient,
    store: &Store,
    renderer: &Mutex<Renderer>,
    req: NavigateReq,
) -> Result<PageDto, String> {
    let raw = req.url.trim();
    let mut url = Url::parse(raw)
        .or_else(|_| Url::parse(&format!("https://{raw}")))
        .map_err(|e| e.to_string())?;
    if settings.shield.clean_urls {
        url = urlclean::clean_for_navigation(&url).0;
    }
    let host = url.host_str().unwrap_or_default().to_string();
    let shield = settings.shield_for(&host);
    let ctx = RequestContext::page(req.container.clone(), shield);
    let before = client.stats_snapshot();
    let started = Instant::now();

    let fetched = match client.fetch(&url, &ctx) {
        FetchOutcome::Ok(f) => f,
        FetchOutcome::Blocked { host, category } => {
            return Err(format!("Щит заблокировал {host} · {}", category.label()))
        }
        FetchOutcome::Error(e) => return Err(e),
    };

    let final_url = fetched.url.clone();
    let html = decode_body(&fetched.body, &fetched.content_type);
    if html.trim().is_empty() && fetched.status >= 400 {
        return Err(format!("Сервер ответил кодом {}", fetched.status));
    }

    let mut loader = Loader {
        client,
        ctx: ctx.clone(),
        top: final_url.clone(),
        cache: HashMap::new(),
    };
    let parsed = Page::parse(html.as_bytes(), final_url.clone(), &settings.custom_css, &mut loader);
    let width = req.width.clamp(640.0, 1800.0);
    let zoom = req.zoom.clamp(0.5, 2.5);
    let out = renderer
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .render(
            &parsed,
            &RenderOptions {
                width,
                zoom,
                reader: req.reader,
            },
            &mut loader,
        );

    let mut rgba = out.pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            px[0] = ((px[0] as u32 * 255) / a).min(255) as u8;
            px[1] = ((px[1] as u32 * 255) / a).min(255) as u8;
            px[2] = ((px[2] as u32 * 255) / a).min(255) as u8;
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &rgba,
            out.width,
            out.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Не удалось закодировать страницу: {e}"))?;
    let png = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );

    let after = client.stats_snapshot();
    let title = if out.title.trim().is_empty() {
        host.clone()
    } else {
        out.title.clone()
    };

    if fetched.status < 400 {
        let _ = store.append_history(&HistoryEntry {
            ts: util::now_ts(),
            url: final_url.to_string(),
            title: title.clone(),
            container: req.container.clone(),
            visits: 1,
        });
    }

    Ok(PageDto {
        url: final_url.to_string(),
        title,
        png,
        width: out.width,
        height: out.height,
        css_width: out.width as f32 / zoom,
        css_height: out.height as f32 / zoom,
        links: out
            .links
            .iter()
            .map(|l| LinkDto {
                x: l.rect.x,
                y: l.rect.y,
                w: l.rect.w,
                h: l.rect.h,
                url: l.url.clone(),
            })
            .collect(),
        lines: out
            .lines
            .iter()
            .map(|l| LineDto {
                x: l.rect.x,
                y: l.rect.y,
                w: l.rect.w,
                h: l.rect.h,
                text: l.text.clone(),
            })
            .collect(),
        text: out.text.clone(),
        js_heavy: out.js_heavy,
        reader: req.reader,
        status: fetched.status,
        truncated: fetched.truncated,
        elapsed_ms: started.elapsed().as_millis(),
        blocked: delta(&before, &after),
        container: req.container,
    })
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    *state.settings.lock().unwrap() = settings.clone();
    *state.shield.lock().unwrap_or_else(|e| e.into_inner()) = settings.shield.clone();
    state
        .store
        .save_settings(&settings)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_history(state: State<'_, AppState>) -> Vec<HistoryEntry> {
    state.store.history(500)
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.store.clear_history().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_bookmark(
    state: State<'_, AppState>,
    url: String,
    title: String,
    container: String,
) -> Result<Vec<Bookmark>, String> {
    let mut list = state.store.load_bookmarks();
    if !list.iter().any(|b| b.url == url) {
        list.push(Bookmark {
            id: uuid::Uuid::new_v4().to_string(),
            url,
            title,
            container,
            created_at: util::now_ts(),
        });
        state.store.save_bookmarks(&list).map_err(|e| e.to_string())?;
    }
    Ok(list)
}

#[tauri::command]
fn remove_bookmark(state: State<'_, AppState>, id: String) -> Result<Vec<Bookmark>, String> {
    let mut list = state.store.load_bookmarks();
    list.retain(|b| b.id != id);
    state.store.save_bookmarks(&list).map_err(|e| e.to_string())?;
    Ok(list)
}

#[tauri::command]
fn save_session(state: State<'_, AppState>, tabs: Vec<TabSession>) -> Result<(), String> {
    state.store.save_session(&tabs).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_container(
    state: State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Vec<Container>, String> {
    let mut settings = state.settings.lock().unwrap();
    settings.containers.push(Container {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        color,
        icon: "cube".into(),
        ephemeral: false,
        created_at: util::now_ts(),
    });
    state.store.save_settings(&settings).map_err(|e| e.to_string())?;
    Ok(settings.containers.clone())
}

#[tauri::command]
fn remove_container(state: State<'_, AppState>, id: String) -> Result<Vec<Container>, String> {
    let mut settings = state.settings.lock().unwrap();
    settings
        .containers
        .retain(|c| c.id != id || c.id == "personal" || c.id == "private");
    state.store.save_settings(&settings).map_err(|e| e.to_string())?;
    let containers = settings.containers.clone();
    drop(settings);

    let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(view) = pages.views.remove(&id) {
        let _ = view.webview.clear_all_browsing_data();
        let _ = view.webview.close();
        let _ = std::fs::remove_dir_all(&view.directory);
    }
    if pages.visible.as_deref() == Some(id.as_str()) {
        pages.visible = None;
    }
    Ok(containers)
}

#[tauri::command]
fn list_passwords(state: State<'_, AppState>) -> Vec<PasswordEntry> {
    state.vault.lock().unwrap().entries().to_vec()
}

#[tauri::command]
fn add_password(
    state: State<'_, AppState>,
    site: String,
    username: String,
    password: String,
) -> Result<Vec<PasswordEntry>, String> {
    let mut vault = state.vault.lock().unwrap();
    vault.add(&site, &username, &password);
    vault.save().map_err(|e| e.to_string())?;
    Ok(vault.entries().to_vec())
}

#[tauri::command]
fn remove_password(state: State<'_, AppState>, id: String) -> Result<Vec<PasswordEntry>, String> {
    let mut vault = state.vault.lock().unwrap();
    vault.remove(&id);
    vault.save().map_err(|e| e.to_string())?;
    Ok(vault.entries().to_vec())
}

#[derive(Serialize, Clone)]
struct PageEvent {
    container: String,
    url: String,
    event: &'static str,
}

#[derive(Serialize, Clone)]
struct TitleEvent {
    container: String,
    title: String,
}

#[derive(Serialize)]
struct NativeDto {
    url: String,
    title: String,
    container: String,
}

fn sanitize_label(container: &str) -> String {
    let mut hash: u64 = 1469598103934665603;
    for byte in container.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("page-{hash:x}")
}

fn page_dir(state: &AppState, container: &str, ephemeral: bool) -> std::path::PathBuf {
    let dir = state.store.root().join("webviews");
    if ephemeral {
        dir.join(format!(".ephemeral-{}", sanitize_label(container)))
    } else {
        dir.join(sanitize_label(container))
    }
}

fn ensure_page(
    app: &tauri::AppHandle,
    state: &AppState,
    container: &str,
) -> Result<(String, tauri::Webview), String> {
    let _creating = state.page_create.lock().unwrap_or_else(|e| e.into_inner());
    {
        let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(view) = pages.views.get(container) {
            return Ok((view.label.clone(), view.webview.clone()));
        }
    }

    let window = app
        .get_window("main")
        .ok_or_else(|| "Главное окно недоступно".to_string())?;
    let settings = state.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let ephemeral = settings
        .container(container)
        .map(|c| c.ephemeral)
        .unwrap_or(container == "private");
    let directory = page_dir(state, container, ephemeral);
    let _ = std::fs::create_dir_all(&directory);

    let label = sanitize_label(container);
    let blank = Url::parse("about:blank").map_err(|e| e.to_string())?;
    let proxy = Url::parse(&format!("http://127.0.0.1:{}", state.proxy_port))
        .map_err(|e| e.to_string())?;

    let container_event = container.to_string();
    let container_title = container.to_string();
    let builder = tauri::webview::WebviewBuilder::new(&label, tauri::WebviewUrl::External(blank))
        .user_agent(PAGE_UA)
        .data_directory(directory.clone())
        .incognito(ephemeral)
        .proxy_url(proxy)
        .initialization_script(PAGE_SCRIPT)
        .background_color(tauri::webview::Color(255, 255, 255, 255))
        .on_page_load(move |webview, payload| {
            let _ = webview.app_handle().emit(
                "rivus-page",
                PageEvent {
                    container: container_event.clone(),
                    url: payload.url().to_string(),
                    event: match payload.event() {
                        tauri::webview::PageLoadEvent::Started => "started",
                        tauri::webview::PageLoadEvent::Finished => "finished",
                    },
                },
            );
        })
        .on_document_title_changed(move |webview, title| {
            let _ = webview.app_handle().emit(
                "rivus-title",
                TitleEvent {
                    container: container_title.clone(),
                    title,
                },
            );
        });

    let (x, y, w, h) = {
        let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages.bounds
    };
    let webview = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(w, h),
        )
        .map_err(|e| format!("Не удалось создать поверхность страницы: {e}"))?;
    let _ = webview.hide();

    {
        let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages.views.insert(
            container.to_string(),
            PageView {
                webview: webview.clone(),
                label: label.clone(),
                ephemeral,
                directory,
            },
        );
    }
    Ok((label, webview))
}

fn show_page(state: &AppState, container: &str) {
    let (bounds, views) = {
        let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        if pages.visible.as_deref() == Some(container) {
            return;
        }
        pages.visible = Some(container.to_string());
        let bounds = pages.bounds;
        let views: Vec<(String, tauri::Webview)> = pages
            .views
            .iter()
            .map(|(id, view)| (id.clone(), view.webview.clone()))
            .collect();
        (bounds, views)
    };
    for (id, webview) in views {
        if id == container {
            let _ = webview.set_position(tauri::LogicalPosition::new(bounds.0, bounds.1));
            let _ = webview.set_size(tauri::LogicalSize::new(bounds.2, bounds.3));
            let _ = webview.show();
            let _ = webview.set_focus();
        } else {
            let _ = webview.hide();
        }
    }
}

fn hide_pages(state: &AppState) {
    let views: Vec<tauri::Webview> = {
        let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages.visible = None;
        pages
            .views
            .values()
            .map(|view| view.webview.clone())
            .collect()
    };
    for webview in views {
        let _ = webview.hide();
    }
}

#[tauri::command]
async fn native_navigate(
    app: tauri::AppHandle,
    url: String,
    container: String,
) -> Result<NativeDto, String> {
    tauri::async_runtime::spawn_blocking(move || native_navigate_blocking(&app, &url, &container))
        .await
        .map_err(|e| e.to_string())?
}

fn native_navigate_blocking(
    app: &tauri::AppHandle,
    url: &str,
    container: &str,
) -> Result<NativeDto, String> {
    let state = app.state::<AppState>();
    let state = state.inner();
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let parsed = Url::parse(url)
        .or_else(|_| Url::parse(&format!("https://{url}")))
        .map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Поддерживаются только http(s)-адреса".into());
    }
    let target = if settings.shield.clean_urls {
        urlclean::clean_for_navigation(&parsed).0
    } else {
        parsed
    };
    let (_, webview) = ensure_page(app, state, container)?;
    webview.navigate(target.clone()).map_err(|e| e.to_string())?;
    show_page(state, container);

    let host = target.host_str().unwrap_or_default().to_string();
    let _ = state.store.append_history(&HistoryEntry {
        ts: util::now_ts(),
        url: target.to_string(),
        title: host.clone(),
        container: container.to_string(),
        visits: 1,
    });

    Ok(NativeDto {
        url: target.to_string(),
        title: host,
        container: container.to_string(),
    })
}

#[tauri::command]
fn page_bounds(state: State<'_, AppState>, x: f64, y: f64, w: f64, h: f64) {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let views: Vec<tauri::Webview> = {
        let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages.bounds = (x, y, w, h);
        pages
            .views
            .values()
            .map(|view| view.webview.clone())
            .collect()
    };
    for webview in views {
        let _ = webview.set_position(tauri::LogicalPosition::new(x, y));
        let _ = webview.set_size(tauri::LogicalSize::new(w, h));
    }
}

#[tauri::command]
fn page_visible(state: State<'_, AppState>, visible: bool, container: Option<String>) {
    if visible {
        if let Some(container) = container {
            show_page(&state, &container);
        }
    } else {
        hide_pages(&state);
    }
}

#[tauri::command]
fn page_activate(state: State<'_, AppState>, container: String, url: Option<String>) -> Result<(), String> {
    let current = {
        let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages
            .views
            .get(&container)
            .and_then(|v| v.webview.url().ok())
            .map(|u| u.to_string())
    };
    let (_, webview) = {
        let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
        match pages.views.get(&container) {
            Some(v) => (v.label.clone(), v.webview.clone()),
            None => return Ok(()),
        }
    };
    if let Some(url) = url {
        if current.as_deref() != Some(url.as_str()) && url != "about:blank" {
            let parsed = Url::parse(&url).map_err(|e| e.to_string())?;
            webview.navigate(parsed).map_err(|e| e.to_string())?;
        }
    }
    show_page(&state, &container);
    Ok(())
}

#[tauri::command]
fn page_action(state: State<'_, AppState>, action: String) -> Result<(), String> {
    let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    let Some(view) = pages.visible.as_ref().and_then(|c| pages.views.get(c)) else {
        return Ok(());
    };
    let js = match action.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        "stop" => "window.stop()",
        "find" => "window.__rivusFind && window.__rivusFind()",
        _ => return Ok(()),
    };
    view.webview.eval(js).map_err(|e| e.to_string())
}

#[tauri::command]
fn page_zoom(state: State<'_, AppState>, zoom: f64) {
    let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(view) = pages.visible.as_ref().and_then(|c| pages.views.get(c)) {
        let _ = view.webview.set_zoom(zoom.clamp(0.5, 2.5));
    }
}

#[tauri::command]
fn page_find(state: State<'_, AppState>, query: String, forward: bool) -> bool {
    let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    let Some(view) = pages.visible.as_ref().and_then(|c| pages.views.get(c)) else {
        return false;
    };
    let q = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".into());
    let js = format!("(function(){{try{{return window.find({q},false,{forward},true,false,false,false)}}catch(e){{return false}}}})()");
    let _ = view.webview.eval(js);
    true
}

#[tauri::command]
fn page_clear(state: State<'_, AppState>, container: Option<String>) -> Result<(), String> {
    let mut pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    let keys: Vec<String> = match container {
        Some(id) => vec![id],
        None => pages.views.keys().cloned().collect(),
    };
    for key in keys {
        if let Some(view) = pages.views.remove(&key) {
            let _ = view.webview.clear_all_browsing_data();
            let _ = view.webview.close();
            let _ = std::fs::remove_dir_all(&view.directory);
        }
    }
    if let Some(visible) = pages.visible.clone() {
        if !pages.views.contains_key(&visible) {
            pages.visible = None;
        }
    }
    Ok(())
}

#[tauri::command]
fn shield_stats(state: State<'_, AppState>) -> ShieldStats {
    state.client.stats_snapshot()
}

fn clear_page_storage(state: &AppState) {
    let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
    for view in pages.views.values() {
        let _ = view.webview.clear_all_browsing_data();
    }
}

#[tauri::command]
fn clear_data(state: State<'_, AppState>, what: String) -> Result<(), String> {
    match what.as_str() {
        "history" => state.store.wipe(Wipe::History),
        "bookmarks" => state.store.wipe(Wipe::Bookmarks),
        "cookies" => {
            state.client.jar.clear_all();
            clear_page_storage(&state);
            state.store.wipe(Wipe::Cookies)
        }
        "passwords" => {
            let mut vault = state.vault.lock().unwrap();
            vault.clear();
            let _ = vault.save();
            state.store.wipe(Wipe::Passwords)
        }
        "everything" => {
            state.client.jar.clear_all();
            state.client.jar.save(&state.store.cookies_path());
            clear_page_storage(&state);
            state.store.wipe(Wipe::Everything)
        }
        _ => Ok(()),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Можно открыть только http(s)-ссылку".into());
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn window_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_update() -> Result<update::UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(update::check)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn install_update(state: State<'_, AppState>) -> Result<String, String> {
    let directory = state.store.root().join("update");
    tauri::async_runtime::spawn_blocking(move || {
        let info = update::check()?;
        let path = update::download(&info, &directory)?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn apply_update(state: State<'_, AppState>) -> Result<(), String> {
    update::apply(&state.store.root().join("update").join("rivus-new.exe"))
}

#[tauri::command]
fn frontend_log(message: String) {
    #[cfg(debug_assertions)]
    eprintln!("[frontend] {message}");
}

fn main() {
    let store = Arc::new(Store::open().expect("Rivus needs a writable data folder"));
    let mut settings = store.load_settings();
    if settings.accent.eq_ignore_ascii_case("#6EE7B7") {
        settings.accent = "#5B9DFF".into();
    }
    for engine in &mut settings.engines {
        match engine.id.as_str() {
            "ddg" if !engine.template.contains("html.duckduckgo.com") => {
                engine.template = "https://html.duckduckgo.com/html/?q=%s".into();
            }
            "google" if !engine.template.contains("gbv=1") => {
                engine.template = "https://www.google.com/search?q=%s&gbv=1".into();
            }
            _ => {}
        }
    }
    let _ = store.save_settings(&settings);
    let filter = aegis_core::FilterEngine::from_data_dir();
    let client = Arc::new(HttpClient::new(filter));
    client.jar.load(&store.cookies_path());
    let vault = Vault::open(store.root());
    let shield = Arc::new(Mutex::new(settings.shield.clone()));
    let proxy_port = proxy::start(client.clone(), shield.clone())
        .expect("Rivus could not start its filtering proxy");
    let _ = std::fs::create_dir_all(store.root().join("webviews"));
    for entry in std::fs::read_dir(store.root().join("webviews")).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(".ephemeral-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }

    tauri::Builder::default()
        .setup(move |app| {
            app.manage(AppState {
                store: store.clone(),
                settings: Mutex::new(settings.clone()),
                shield: shield.clone(),
                client: client.clone(),
                vault: Mutex::new(vault),
                renderer: Arc::new(Mutex::new(Renderer::new())),
                pages: Mutex::new(Pages::default()),
                page_create: Mutex::new(()),
                proxy_port,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            navigate,
            native_navigate,
            page_bounds,
            page_visible,
            page_activate,
            page_action,
            page_zoom,
            page_find,
            page_clear,
            shield_stats,
            save_settings,
            list_history,
            clear_history,
            add_bookmark,
            remove_bookmark,
            save_session,
            add_container,
            remove_container,
            list_passwords,
            add_password,
            remove_password,
            clear_data,
            open_external,
            window_minimize,
            window_maximize,
            window_close,
            check_update,
            install_update,
            apply_update,
            frontend_log
        ])
        .build(tauri::generate_context!())
        .expect("error while building Rivus")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let pages = state.pages.lock().unwrap_or_else(|e| e.into_inner());
                    for view in pages.views.values() {
                        if view.ephemeral {
                            let _ = std::fs::remove_dir_all(&view.directory);
                        }
                    }
                }
            }
        });
}
