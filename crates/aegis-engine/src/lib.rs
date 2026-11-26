//! Aegis rendering engine.
//!
//! A small, dependency-light document engine: HTML is parsed with html5ever,
//! styled with a hand-written CSS subset, laid out with cosmic-text and
//! rasterised with tiny-skia. No JavaScript is executed and nothing is cached
//! on disk.

pub mod css;
pub mod dom;
pub mod layout;
pub mod paint;
pub mod reader;
pub mod style;

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::Pixmap;
use url::Url;

pub use css::{parse_stylesheet, Rule};
pub use dom::{parse_html, Dom};
pub use layout::{
    layout_page, Layout, LayoutOptions, LineBox, LinkBox, LoadedImage, Loader, Rect,
};
pub use paint::{paint, ImageCache};
pub use style::{compute_styles, Style};

/// A subresource (stylesheet, image) returned by a [`Fetcher`].
#[derive(Debug, Clone)]
pub struct FetchedResource {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

/// Hook the browser implements to load subresources through its privacy stack.
pub trait Fetcher {
    fn fetch(&mut self, url: &Url) -> Option<FetchedResource>;
}

impl Fetcher for () {
    fn fetch(&mut self, _url: &Url) -> Option<FetchedResource> {
        None
    }
}

/// A stylesheet larger than this is ignored.
const MAX_CSS_BYTES: usize = 2 * 1024 * 1024;

/// A parsed document with style information attached.
pub struct Page {
    pub url: Url,
    pub base: Url,
    pub dom: Dom,
    pub rules: Vec<Rule>,
    pub styles: Vec<Style>,
    pub title: String,
    pub js_heavy: bool,
    pub text: String,
}

impl Page {
    /// Parse HTML, collect stylesheets (blocked subresources simply vanish) and
    /// compute styles. `extra_css` is appended after the page's own CSS.
    pub fn parse(
        html: &[u8],
        url: Url,
        extra_css: &str,
        fetcher: &mut dyn Fetcher,
    ) -> Self {
        let dom = parse_html(html);
        let base = dom
            .find_first("base")
            .and_then(|id| dom.attr(id, "href"))
            .and_then(|href| url.join(href).ok())
            .unwrap_or_else(|| url.clone());

        let mut css = String::new();
        let mut fetched: usize = 0;
        for id in dom.document_order() {
            let Some(el) = dom.element(id) else { continue };
            if el.tag == "style" {
                css.push_str(&dom.text_content(id));
                css.push('\n');
            } else if el.tag == "link" {
                let rel = el.attr("rel").unwrap_or("").to_ascii_lowercase();
                if !rel.split_whitespace().any(|r| r == "stylesheet") {
                    continue;
                }
                let Some(href) = el.attr("href") else { continue };
                if fetched >= 8 {
                    continue;
                }
                let Ok(href_url) = base.join(href) else { continue };
                if !matches!(href_url.scheme(), "http" | "https" | "data") {
                    continue;
                }
                fetched += 1;
                if let Some(res) = fetcher.fetch(&href_url) {
                    if res.bytes.len() <= MAX_CSS_BYTES {
                        css.push_str(&String::from_utf8_lossy(&res.bytes));
                        css.push('\n');
                    }
                }
            }
        }
        if !extra_css.trim().is_empty() {
            css.push('\n');
            css.push_str(extra_css);
        }

        let rules = parse_stylesheet(&css);
        let styles = compute_styles(&dom, &rules);
        let title = dom.title();
        let text = layout::document_text(&dom, &styles);
        let js_heavy = reader::is_js_heavy(&dom, &styles);

        Self {
            url,
            base,
            dom,
            rules,
            styles,
            title,
            js_heavy,
            text,
        }
    }

    /// Re-style the document, e.g. after toggling reader mode or changing the
    /// user stylesheet.
    pub fn restyle(&mut self) {
        self.styles = compute_styles(&self.dom, &self.rules);
    }
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Device pixel width of the viewport.
    pub width: f32,
    pub zoom: f32,
    pub reader: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 1100.0,
            zoom: 1.0,
            reader: false,
        }
    }
}

pub struct RenderOutput {
    pub pixmap: Pixmap,
    pub links: Vec<LinkBox>,
    pub lines: Vec<LineBox>,
    pub text: String,
    pub title: String,
    pub js_heavy: bool,
    pub width: u32,
    pub height: u32,
}

/// Holds the font system, glyph cache and decoded image cache for a tab.
pub struct Renderer {
    pub font_system: FontSystem,
    pub swash: SwashCache,
    pub images: ImageCache,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
            images: HashMap::new(),
        }
    }

    pub fn render(
        &mut self,
        page: &Page,
        opt: &RenderOptions,
        fetcher: &mut dyn Fetcher,
    ) -> RenderOutput {
        let zoom = opt.zoom.clamp(0.25, 4.0);
        let device_width = opt.width.max(240.0);
        let css_width = device_width / zoom;

        let mut reader_doc: Option<(Dom, Vec<Style>)> = None;
        let mut reader_title: Option<String> = None;
        if opt.reader {
            let (title, html) = reader::reader_document(&page.dom, &page.styles);
            let dom = parse_html(html.as_bytes());
            let rules = parse_stylesheet(READER_CSS);
            let styles = compute_styles(&dom, &rules);
            reader_title = Some(title);
            reader_doc = Some((dom, styles));
        }
        let (dom, styles) = match &reader_doc {
            Some((dom, styles)) => (dom, styles),
            None => (&page.dom, &page.styles),
        };

        let layout_opt = LayoutOptions {
            width: css_width,
            zoom,
            base: page.base.clone(),
        };
        let mut loader = FetchLoader {
            fetcher,
            cache: HashMap::new(),
        };
        let layout = layout_page(
            dom,
            styles,
            &layout_opt,
            &mut self.font_system,
            &mut loader,
        );

        let width = device_width.round().max(1.0) as u32;
        let height = (layout.height.ceil() as u32).clamp(1, 24_000);
        let pixmap = paint(
            &layout,
            width,
            height,
            &mut self.font_system,
            &mut self.swash,
            &mut self.images,
        )
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"));

        let title = reader_title.unwrap_or_else(|| page.title.clone());

        RenderOutput {
            pixmap,
            links: layout.links,
            lines: layout.lines,
            text: layout.text,
            title,
            js_heavy: page.js_heavy && !opt.reader,
            width,
            height,
        }
    }
}

/// Minimal user-agent styling for the reader-mode document.
const READER_CSS: &str = "
body{margin:0;padding:40px 32px;background:#ffffff;font-family:sans-serif;font-size:19px;line-height:1.7}
.reader{max-width:720px}
h1{font-size:2em;font-weight:700;margin:0 0 20px}
h2{font-size:1.45em;font-weight:700;margin:28px 0 8px}
p{margin:0 0 18px}
";

struct FetchLoader<'a> {
    fetcher: &'a mut dyn Fetcher,
    cache: HashMap<String, Option<LoadedImage>>,
}

impl Loader for FetchLoader<'_> {
    fn image(&mut self, url: &Url) -> Option<LoadedImage> {
        let key = url.to_string();
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let loaded = self.fetcher.fetch(url).and_then(|res| {
            let reader = image::ImageReader::new(Cursor::new(res.bytes.as_slice()))
                .with_guessed_format()
                .ok()?;
            let (width, height) = reader.into_dimensions().ok()?;
            if width == 0 || height == 0 {
                return None;
            }
            Some(LoadedImage {
                bytes: Arc::new(res.bytes),
                width,
                height,
            })
        });
        self.cache.insert(key, loaded.clone());
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html>
<html><head><title>Test page</title>
<style>
body { margin: 0; padding: 24px; background: #fdfdfc; }
h1 { font-size: 40px; margin: 0 0 12px; color: #112233; }
p { margin: 0 0 16px; color: #333; }
.hint { color: #a33; }
</style></head>
<body>
<h1>Hello Aegis</h1>
<p>This is a <b>rendered</b> paragraph with a <a href="https://example.com/x">link</a>.</p>
<p class="hint">Second paragraph.</p>
<ul><li>One</li><li>Two</li></ul>
<pre>let x = 1;</pre>
</body></html>"#;

    fn render_html(html: &str, width: f32) -> RenderOutput {
        let url = Url::parse("https://example.com/").unwrap();
        let page = Page::parse(html.as_bytes(), url, "", &mut ());
        let mut renderer = Renderer::new();
        renderer.render(
            &page,
            &RenderOptions {
                width,
                zoom: 1.0,
                reader: false,
            },
            &mut (),
        )
    }

    #[test]
    fn renders_a_page() {
        let out = render_html(PAGE, 800.0);
        assert_eq!(out.title, "Test page");
        assert!(out.height > 100, "height was {}", out.height);
        assert!(out.width == 800);
        assert!(!out.links.is_empty(), "expected a link box");
        assert!(out.text.contains("Second paragraph"));
        // Check that something was actually painted: the heading is dark blue.
        let data = out.pixmap.data();
        let mut colored = 0;
        for px in data.chunks_exact(4) {
            if px[0] < 120 && px[1] < 120 && px[2] < 140 && px[3] == 255 {
                colored += 1;
            }
        }
        assert!(colored > 100, "expected painted text pixels, got {colored}");
    }

    #[test]
    fn reader_mode_extracts_article() {
        let out = render_html(PAGE, 700.0);
        assert!(out.height > 50);
        let url = Url::parse("https://example.com/").unwrap();
        let page = Page::parse(PAGE.as_bytes(), url, "", &mut ());
        let mut renderer = Renderer::new();
        let read = renderer.render(
            &page,
            &RenderOptions {
                width: 700.0,
                zoom: 1.0,
                reader: true,
            },
            &mut (),
        );
        assert!(!read.text.trim().is_empty());
    }

    #[test]
    fn zoom_increases_height() {
        let normal = render_html(PAGE, 800.0);
        let url = Url::parse("https://example.com/").unwrap();
        let page = Page::parse(PAGE.as_bytes(), url, "", &mut ());
        let mut renderer = Renderer::new();
        let zoomed = renderer.render(
            &page,
            &RenderOptions {
                width: 800.0,
                zoom: 2.0,
                reader: false,
            },
            &mut (),
        );
        assert!(zoomed.height > normal.height);
    }
}
