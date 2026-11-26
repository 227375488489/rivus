//! Block and inline layout.
//!
//! The engine implements a deliberately small subset of CSS layout: normal
//! block flow with wrapped rich-text inline boxes. Floats, absolute
//! positioning, flex and grid are not supported; documents that depend on
//! them degrade to a readable linear flow.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Weight,
    Wrap,
};
use url::Url;

use crate::dom::{Dom, NodeData, NodeId};
use crate::style::{Align, Display, FontFamily, ListStyle, Rgba, Style, WhiteSpace};

/// Hard ceiling for a rendered document, keeps memory bounded on runaway pages.
pub const MAX_DOC_HEIGHT: f32 = 24_000.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub rect: Rect,
    pub color: Rgba,
    pub radius: f32,
}

#[derive(Debug, Clone, Default)]
pub struct GlyphMeta {
    pub link: Option<usize>,
    pub underline: bool,
    pub strike: bool,
}

#[derive(Debug)]
pub struct TextLayer {
    pub buffer: Buffer,
    pub x: f32,
    pub y: f32,
    pub meta: Vec<GlyphMeta>,
}

#[derive(Debug, Clone)]
pub struct LinkBox {
    pub rect: Rect,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct ImageBox {
    pub rect: Rect,
    pub url: String,
    pub bytes: Option<Arc<Vec<u8>>>,
    pub alt: String,
}

/// A shaped line of text, used for in-page find and text selection outlines.
#[derive(Debug, Clone)]
pub struct LineBox {
    pub rect: Rect,
    pub text: String,
}

#[derive(Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub page_bg: Rgba,
    pub fills: Vec<Fill>,
    pub images: Vec<ImageBox>,
    pub texts: Vec<TextLayer>,
    pub links: Vec<LinkBox>,
    pub lines: Vec<LineBox>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LayoutOptions {
    pub width: f32,
    pub zoom: f32,
    pub base: Url,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            width: 1000.0,
            zoom: 1.0,
            base: Url::parse("about:blank").unwrap(),
        }
    }
}

/// Decoded image header information plus the original bytes.
#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub bytes: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// Resource hook used while laying out to fetch images.
pub trait Loader {
    fn image(&mut self, url: &Url) -> Option<LoadedImage>;
}

impl Loader for () {
    fn image(&mut self, _url: &Url) -> Option<LoadedImage> {
        None
    }
}

#[derive(Debug)]
struct Span {
    text: String,
    attrs: Attrs<'static>,
}

#[derive(Default)]
struct InlineCollector {
    spans: Vec<Span>,
    meta: Vec<GlyphMeta>,
    plain: String,
    last_space: bool,
    link: Option<usize>,
    started: bool,
}

struct Layouter<'a> {
    dom: &'a Dom,
    styles: &'a [Style],
    opt: LayoutOptions,
    font_system: &'a mut FontSystem,
    loader: &'a mut dyn Loader,
    out: Layout,
    link_ids: Vec<String>,
    link_lookup: HashMap<String, usize>,
    images: HashMap<String, Option<LoadedImage>>,
    depth: usize,
}

/// Lay out a parsed document into a display list.
pub fn layout_page(
    dom: &Dom,
    styles: &[Style],
    opt: &LayoutOptions,
    font_system: &mut FontSystem,
    loader: &mut dyn Loader,
) -> Layout {
    let root = dom.body().or_else(|| dom.find_first("html")).unwrap_or(dom.root);
    let root_style = styles.get(root).cloned().unwrap_or_default();
    let html_style = dom
        .find_first("html")
        .and_then(|id| styles.get(id).cloned())
        .unwrap_or_default();

    let width = opt.width.max(200.0);
    let page_bg = root_style
        .background
        .or(html_style.background)
        .unwrap_or([255, 255, 255, 255]);

    let mut lay = Layouter {
        dom,
        styles,
        opt: opt.clone(),
        font_system,
        loader,
        out: Layout {
            width,
            height: 0.0,
            page_bg,
            fills: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            links: Vec::new(),
            lines: Vec::new(),
            text: String::new(),
        },
        link_ids: Vec::new(),
        link_lookup: HashMap::new(),
        images: HashMap::new(),
        depth: 0,
    };

    let zoom = lay.opt.zoom.max(0.25);
    let margin = lay.scale_edges(&root_style.margin, zoom);
    let x = margin.left;
    let avail = (width - margin.left - margin.right).max(160.0);
    let height = lay.block(root, x, margin.top, avail);

    let mut y = margin.top + height + margin.bottom;
    if root_style.background.is_none() {
        // Many pages paint the page colour on <html> only.
        if let Some(bg) = html_style.background {
            lay.out.page_bg = bg;
        }
    }
    y = y.min(MAX_DOC_HEIGHT);
    lay.out.height = y;
    lay.out
}

impl<'a> Layouter<'a> {
    fn style(&self, id: NodeId) -> Style {
        self.styles.get(id).cloned().unwrap_or_default()
    }

    fn scale_edges(&self, e: &crate::style::Edges, zoom: f32) -> crate::style::Edges {
        crate::style::Edges {
            top: e.top * zoom,
            right: e.right * zoom,
            bottom: e.bottom * zoom,
            left: e.left * zoom,
        }
    }

    fn resolve(&self, len: &Option<crate::style::Len>, base: f32, em: f32, zoom: f32) -> Option<f32> {
        len.as_ref().and_then(|l| l.resolve(base, em)).map(|v| v * zoom)
    }

    fn box_width(&self, s: &Style, avail: f32, zoom: f32) -> f32 {
        let pad = self.scale_edges(&s.padding, zoom);
        let bord = self.scale_edges(&s.border, zoom);
        let extra = pad.left + pad.right + bord.left + bord.right;
        let em = s.font_size * zoom;
        let mut w = if let Some(v) = self.resolve(&s.width, avail, em, zoom) {
            v
        } else {
            avail
        };
        if let Some(v) = self.resolve(&s.max_width, avail, em, zoom) {
            w = w.min(v);
        }
        if let Some(v) = self.resolve(&s.min_width, avail, em, zoom) {
            w = w.max(v);
        }
        w.max(extra + 8.0).min(avail.max(extra + 8.0))
    }

    /// Lay out a block box whose border box starts at `(x, y)` inside `avail`.
    /// Returns the border-box height. Margins are handled by the caller.
    fn block(&mut self, id: NodeId, x: f32, y: f32, avail: f32) -> f32 {
        let s = self.style(id);
        let zoom = self.opt.zoom.max(0.25);
        if s.display == Display::None || !s.visible || self.depth > 200 {
            return 0.0;
        }
        self.depth += 1;

        let pad = self.scale_edges(&s.padding, zoom);
        let bord = self.scale_edges(&s.border, zoom);
        let bw = self.box_width(&s, avail, zoom);
        let content_x = x + bord.left + pad.left;
        let content_w =
            (bw - bord.left - bord.right - pad.left - pad.right).max(8.0);

        let fill_index = if s.background.is_some() || !bord.is_zero() {
            let idx = self.out.fills.len();
            self.out.fills.push(Fill {
                rect: Rect::new(x, y, bw, 0.0),
                color: [0, 0, 0, 0],
                radius: s.radius * zoom,
            });
            Some(idx)
        } else {
            None
        };

        let mut cursor = y + bord.top + pad.top;

        if self.dom.tag(id) == Some("hr") {
            let h = (bord.top.max(1.0)).max(1.0);
            self.out.fills.push(Fill {
                rect: Rect::new(content_x, cursor, content_w, h),
                color: if s.border_color[3] == 0 {
                    [200, 200, 200, 255]
                } else {
                    s.border_color
                },
                radius: 0.0,
            });
            cursor += h;
        } else if self.is_replaced(id) {
            cursor = self.block_image(id, &s, content_x, cursor, content_w, zoom);
        } else {
            if s.display == Display::ListItem {
                self.list_marker(id, &s, content_x, cursor, zoom);
            }
            let content_h = self.flow(id, content_x, cursor, content_w, &s, zoom);
            cursor += content_h;
        }

        cursor += pad.bottom + bord.bottom;

        let requested = self
            .resolve(&s.height, 0.0, s.font_size * zoom, zoom)
            .map(|v| v + bord.top + bord.bottom + pad.top + pad.bottom);
        if let Some(min_h) = requested {
            cursor = cursor.max(y + min_h);
        }
        let bh = (cursor - y).max(0.0).min(MAX_DOC_HEIGHT - y);

        if let Some(idx) = fill_index {
            let radius = self.out.fills[idx].radius;
            let (rect, color) = if s.background.is_some() {
                (
                    Rect::new(x, y, bw, bh),
                    s.background.unwrap_or([255, 255, 255, 255]),
                )
            } else {
                (Rect::new(x, y, bw, bh), [0, 0, 0, 0])
            };
            self.out.fills[idx] = Fill {
                rect,
                color,
                radius,
            };
            if !bord.is_zero() {
                self.border_lines(&s, x, y, bw, bh, &bord);
            }
        }

        self.depth -= 1;
        bh
    }

    fn border_lines(
        &mut self,
        s: &Style,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        bord: &crate::style::Edges,
    ) {
        let c = s.border_color;
        if c[3] == 0 {
            return;
        }
        if bord.top > 0.0 {
            self.out
                .fills
                .push(Fill { rect: Rect::new(x, y, w, bord.top), color: c, radius: 0.0 });
        }
        if bord.bottom > 0.0 {
            self.out.fills.push(Fill {
                rect: Rect::new(x, y + h - bord.bottom, w, bord.bottom),
                color: c,
                radius: 0.0,
            });
        }
        if bord.left > 0.0 {
            self.out
                .fills
                .push(Fill { rect: Rect::new(x, y, bord.left, h), color: c, radius: 0.0 });
        }
        if bord.right > 0.0 {
            self.out.fills.push(Fill {
                rect: Rect::new(x + w - bord.right, y, bord.right, h),
                color: c,
                radius: 0.0,
            });
        }
    }

    fn list_marker(&mut self, id: NodeId, s: &Style, x: f32, y: f32, zoom: f32) {
        let marker = match s.list_style {
            ListStyle::None => return,
            ListStyle::Disc => "\u{2022}".to_string(),
            ListStyle::Circle => "\u{25E6}".to_string(),
            ListStyle::Square => "\u{25AA}".to_string(),
            ListStyle::Decimal => {
                let n = self.dom.element_index(id) + 1;
                format!("{n}.")
            }
        };
        let size = (s.font_size * zoom).max(6.0);
        let line = (s.line_px() * zoom).max(size * 1.1);
        let mut buffer = Buffer::new(self.font_system, Metrics::new(size, line));
        buffer.set_size(self.font_system, Some((size * 4.0).max(24.0)), Some(line));
        buffer.set_wrap(self.font_system, Wrap::None);
        let attrs = self.attrs(s, 0, zoom);
        buffer.set_rich_text(
            self.font_system,
            [(marker.as_str(), attrs)],
            &Attrs::new(),
            Shaping::Advanced,
            Some(CosmicAlign::Right),
        );
        buffer.shape_until_scroll(self.font_system, false);
        let offset = (s.font_size * zoom * 1.6).max(14.0);
        self.out.texts.push(TextLayer {
            buffer,
            x: x - offset,
            y,
            meta: vec![GlyphMeta::default()],
        });
    }

    fn is_replaced(&self, id: NodeId) -> bool {
        matches!(self.dom.tag(id), Some("img"))
    }

    fn block_image(
        &mut self,
        id: NodeId,
        s: &Style,
        x: f32,
        y: f32,
        avail: f32,
        zoom: f32,
    ) -> f32 {
        let alt = self.dom.attr(id, "alt").unwrap_or("").to_string();
        let Some(src) = self.dom.attr(id, "src") else {
            return 0.0;
        };
        let Ok(url) = self.opt.base.join(src) else {
            return 0.0;
        };
        if !matches!(url.scheme(), "http" | "https" | "data") {
            return 0.0;
        }
        let key = url.to_string();
        let loaded = if let Some(cached) = self.images.get(&key) {
            cached.clone()
        } else {
            let fetched = self.loader.image(&url);
            self.images.insert(key.clone(), fetched.clone());
            fetched
        };

        let em = s.font_size * zoom;
        let mut w = self
            .resolve(&s.width, avail, em, zoom)
            .unwrap_or_else(|| {
                loaded
                    .as_ref()
                    .map(|l| (l.width as f32 * zoom).min(avail))
                    .unwrap_or(avail.min(480.0))
            })
            .min(avail);
        let mut h = self
            .resolve(&s.height, 0.0, em, zoom)
            .or_else(|| {
                loaded.as_ref().map(|l| {
                    let ratio = l.height as f32 / l.width.max(1) as f32;
                    w * ratio
                })
            })
            .unwrap_or(160.0);
        if w < 8.0 {
            w = avail.min(480.0);
        }
        if h < 8.0 {
            h = 120.0;
        }
        if let Some(l) = &loaded {
            if s.height.is_none() && s.width.is_none() {
                let nw = l.width as f32 * zoom;
                let nh = l.height as f32 * zoom;
                if nw > 0.0 && nw <= avail {
                    w = nw;
                    h = nh;
                }
            }
        }
        let h = h.min(MAX_DOC_HEIGHT - y).max(0.0);
        self.out.images.push(ImageBox {
            rect: Rect::new(x, y, w, h),
            url: key,
            bytes: loaded.map(|l| l.bytes),
            alt,
        });
        h
    }

    fn flow(
        &mut self,
        parent: NodeId,
        cx: f32,
        cy: f32,
        cw: f32,
        parent_style: &Style,
        zoom: f32,
    ) -> f32 {
        let mut y = cy;
        let mut pending_margin = 0.0f32;
        let mut has_block = false;
        let mut inline = InlineCollector::default();

        for child in self.dom.node(parent).children.clone() {
            if self.out.height + y > MAX_DOC_HEIGHT {
                break;
            }
            let cs = self.style(child);
            if cs.display == Display::None || !cs.visible {
                continue;
            }
            let replaced = self.is_replaced(child);
            if cs.is_block_level() || replaced {
                self.flush_inline(&mut inline, cx, &mut y, cw, parent_style, zoom);
                let m = self.scale_edges(&cs.margin, zoom);
                let top = if has_block { m.top.max(pending_margin) } else { m.top };
                y += top;
                let h = self.block(child, cx, y, cw);
                y += h;
                pending_margin = m.bottom;
                has_block = true;
            } else {
                self.collect_inline(child, &mut inline, parent_style, zoom);
            }
        }
        self.flush_inline(&mut inline, cx, &mut y, cw, parent_style, zoom);
        (y - cy) + pending_margin
    }

    fn collect_inline(
        &mut self,
        id: NodeId,
        inline: &mut InlineCollector,
        inherited: &Style,
        zoom: f32,
    ) {
        match &self.dom.node(id).data {
            NodeData::Text(text) => {
                let s = inherited.clone();
                let raw: String = if s.white_space == WhiteSpace::Pre {
                    text.clone()
                } else {
                    collapse_ws(text)
                };
                let text = if s.uppercase {
                    raw.to_uppercase()
                } else {
                    raw
                };
                if text.is_empty() {
                    return;
                }
                if !inline.started && text.starts_with(char::is_whitespace) {
                    let trimmed = text.trim_start().to_string();
                    if trimmed.is_empty() {
                        return;
                    }
                    self.push_span(inline, trimmed, &s, zoom);
                } else {
                    self.push_span(inline, text, &s, zoom);
                }
            }
            NodeData::Element(el) => {
                if el.tag == "br" {
                    let s = inherited.clone();
                    self.push_span(inline, "\n".to_string(), &s, zoom);
                    return;
                }
                if matches!(
                    el.tag.as_str(),
                    "script" | "style" | "head" | "title" | "meta" | "link" | "template"
                ) {
                    return;
                }
                let s = self.style(id);
                let saved_link = inline.link;
                let saved_started = inline.started;
                if el.tag == "a" {
                    if let Some(href) = el.attr("href") {
                        if !href.starts_with('#') && !href.starts_with("javascript:") {
                            if let Ok(url) = self.opt.base.join(href) {
                                let link = self.link_id(url.as_str());
                                inline.link = Some(link);
                                inline.started = false;
                            }
                        }
                    }
                }
                for child in self.dom.node(id).children.clone() {
                    let cs = self.style(child);
                    if cs.display == Display::None || !cs.visible || self.is_replaced(child) {
                        continue;
                    }
                    self.collect_inline(child, inline, &s, zoom);
                }
                inline.link = saved_link;
                inline.started = saved_started || !inline.spans.is_empty();
            }
            _ => {}
        }
    }

    fn link_id(&mut self, url: &str) -> usize {
        if let Some(id) = self.link_lookup.get(url) {
            return *id;
        }
        let id = self.link_ids.len();
        self.link_ids.push(url.to_string());
        self.link_lookup.insert(url.to_string(), id);
        id
    }

    fn push_span(&mut self, inline: &mut InlineCollector, text: String, s: &Style, zoom: f32) {
        let meta = GlyphMeta {
            link: inline.link,
            underline: s.underline,
            strike: s.strike,
        };
        inline.plain.push_str(&text);
        inline.last_space = text.ends_with(char::is_whitespace);
        inline.started = true;
        let idx = inline.meta.len();
        let attrs = self.attrs(s, idx, zoom);
        inline.spans.push(Span { text, attrs });
        inline.meta.push(meta);
    }

    fn attrs(&self, s: &Style, idx: usize, zoom: f32) -> Attrs<'static> {
        let family = match s.family {
            FontFamily::Sans => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Mono => Family::Monospace,
        };
        let mut c = s.color;
        c[3] = ((c[3] as f32) * s.opacity.clamp(0.0, 1.0)) as u8;
        let mut attrs = Attrs::new()
            .family(family)
            .color(Color::rgba(c[0], c[1], c[2], c[3]))
            .metadata(idx)
            .weight(if s.font_weight >= 600 {
                Weight::BOLD
            } else {
                Weight::NORMAL
            });
        if s.italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }
        let _ = zoom;
        attrs
    }

    fn flush_inline(
        &mut self,
        inline: &mut InlineCollector,
        cx: f32,
        y: &mut f32,
        cw: f32,
        parent: &Style,
        zoom: f32,
    ) {
        if inline.spans.is_empty() {
            *y += 0.0;
            return;
        }
        while inline
            .spans
            .last()
            .map(|s| s.text.trim().is_empty())
            .unwrap_or(false)
        {
            inline.spans.pop();
            inline.meta.pop();
        }
        if inline.spans.is_empty() {
            inline.plain.clear();
            return;
        }
        let line = (parent.line_px() * zoom).max(6.0);
        let size = (parent.font_size * zoom).max(6.0);
        let line_height = line.max(size * 1.15);
        let mut buffer = Buffer::new(self.font_system, Metrics::new(size, line_height));
        buffer.set_size(self.font_system, Some(cw.max(8.0)), None);
        buffer.set_wrap(self.font_system, Wrap::WordOrGlyph);

        let owned: Vec<(String, Attrs<'static>)> = inline
            .spans
            .drain(..)
            .map(|s| (s.text, s.attrs))
            .collect();
        let align = match parent.align {
            Align::Left => Some(CosmicAlign::Left),
            Align::Center => Some(CosmicAlign::Center),
            Align::Right => Some(CosmicAlign::Right),
            Align::Justify => Some(CosmicAlign::Justified),
        };
        buffer.set_rich_text(
            self.font_system,
            owned.iter().map(|(t, a)| (t.as_str(), a.clone())),
            &Attrs::new(),
            Shaping::Advanced,
            align,
        );
        buffer.shape_until_scroll(self.font_system, false);

        let mut text_h = 0.0f32;
        let mut line_boxes = Vec::new();
        for run in buffer.layout_runs() {
            text_h = text_h.max(run.line_top + run.line_height);
            line_boxes.push(LineBox {
                rect: Rect::new(cx, *y + run.line_top, run.line_w, run.line_height),
                text: run.text.to_string(),
            });
        }
        if text_h <= 0.0 {
            inline.plain.clear();
            return;
        }

        let layer_index = self.out.texts.len();
        self.out.texts.push(TextLayer {
            buffer,
            x: cx,
            y: *y,
            meta: inline.meta.drain(..).collect(),
        });
        self.out.lines.extend(line_boxes);
        self.out.text.push_str(&inline.plain);
        self.out.text.push('\n');
        inline.plain.clear();
        inline.started = false;
        inline.last_space = false;

        // Link hit boxes, derived from glyph positions of the shaped buffer.
        let mut link_runs: Vec<(usize, Rect)> = Vec::new();
        if !self.link_ids.is_empty() {
            let layer = &self.out.texts[layer_index];
            let runs = &mut link_runs;
            for run in layer.buffer.layout_runs() {
                let mut current: Option<(usize, f32, f32)> = None;
                for glyph in run.glyphs.iter() {
                    let meta = layer
                        .meta
                        .get(glyph.metadata)
                        .cloned()
                        .unwrap_or_default();
                    let phys = glyph.physical((0.0, run.line_y), 1.0);
                    let px = phys.x as f32;
                    match meta.link {
                        Some(id) => {
                            let start = px;
                            let end = px + glyph.font_size * 0.55;
                            match current {
                                Some((cur, x0, x1)) if cur == id => {
                                    current = Some((cur, x0, x1.max(end)));
                                }
                                Some((cur, x0, x1)) => {
                                    runs.push((
                                        cur,
                                        Rect::new(cx + x0, *y + run.line_top, (x1 - x0).max(1.0), run.line_height),
                                    ));
                                    current = Some((id, start, end));
                                }
                                None => current = Some((id, start, end)),
                            }
                        }
                        None => {
                            if let Some((cur, x0, x1)) = current.take() {
                                runs.push((
                                    cur,
                                    Rect::new(cx + x0, *y + run.line_top, (x1 - x0).max(1.0), run.line_height),
                                ));
                            }
                        }
                    }
                }
                if let Some((cur, x0, x1)) = current.take() {
                    runs.push((
                        cur,
                        Rect::new(cx + x0, *y + run.line_top, (x1 - x0).max(1.0), run.line_height),
                    ));
                }
            }
        }
        for (id, rect) in link_runs {
            if let Some(url) = self.link_ids.get(id).cloned() {
                self.out.links.push(LinkBox { rect, url });
            }
        }

        *y += text_h;
    }
}

fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
        } else {
            out.push(ch);
            space = false;
        }
    }
    out
}

/// Plain-text rendering of the document, used for reader mode and find.
pub fn document_text(dom: &Dom, styles: &[Style]) -> String {
    let mut out = String::new();
    for id in dom.document_order() {
        match &dom.node(id).data {
            NodeData::Text(t) => {
                let s = styles.get(id).cloned().unwrap_or_default();
                if s.visible {
                    out.push_str(t);
                }
            }
            NodeData::Element(el) => {
                if matches!(el.tag.as_str(), "script" | "style" | "head" | "title") {
                    continue;
                }
                if matches!(el.tag.as_str(), "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "tr") {
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    out
}
