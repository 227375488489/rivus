//! Reader mode: pull the main article out of a page and re-render it as a
//! clean, script-free document.

use crate::dom::{Dom, NodeData, NodeId};
use crate::style::{Display, Style};

const SKIP: &[&str] = &[
    "script", "style", "nav", "aside", "footer", "header", "form", "button", "input", "select",
    "textarea", "noscript", "iframe", "svg", "canvas", "template", "menu",
];

fn text_len(dom: &Dom, id: NodeId) -> usize {
    dom.text_content(id).trim().len()
}

/// Pick the element that most likely holds the article body.
fn article_root(dom: &Dom) -> NodeId {
    for tag in ["article", "main"] {
        if let Some(id) = dom.find_first(tag) {
            if text_len(dom, id) > 200 {
                return id;
            }
        }
    }
    if let Some(id) = dom.find_first_with("div", "role") {
        if dom.attr(id, "role") == Some("main") && text_len(dom, id) > 200 {
            return id;
        }
    }

    let mut best = dom.body().unwrap_or(dom.root);
    let mut best_score = 0usize;
    for id in dom.document_order() {
        if let Some(el) = dom.element(id) {
            if !matches!(el.tag.as_str(), "div" | "section" | "td") {
                continue;
            }
            let class = el.attr("class").unwrap_or("").to_ascii_lowercase();
            let id_attr = el.attr("id").unwrap_or("").to_ascii_lowercase();
            let hint = class.contains("content")
                || class.contains("article")
                || class.contains("post")
                || class.contains("entry")
                || class.contains("story")
                || class.contains("text")
                || id_attr.contains("content");
            let len = text_len(dom, id);
            let score = len + if hint { len / 2 } else { 0 };
            if score > best_score {
                best_score = score;
                best = id;
            }
        }
    }
    best
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn collapse(text: &str) -> String {
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

struct Builder {
    out: Vec<String>,
    buf: String,
    heading: Option<u8>,
}

impl Builder {
    fn flush(&mut self) {
        let text = self.buf.trim().to_string();
        self.buf.clear();
        if text.is_empty() {
            self.heading = None;
            return;
        }
        let escaped = escape(&text);
        let tag = match self.heading.take() {
            Some(1) => "h1",
            Some(2) | Some(3) => "h2",
            _ => "p",
        };
        self.out.push(format!("<{tag}>{escaped}</{tag}>"));
    }
}

fn skipped(dom: &Dom, styles: &[Style], id: NodeId) -> bool {
    let mut current = Some(id);
    while let Some(node) = current {
        if let Some(el) = dom.element(node) {
            if SKIP.contains(&el.tag.as_str()) || el.attr("hidden").is_some() {
                return true;
            }
        }
        if styles
            .get(node)
            .map(|s| s.display == Display::None)
            .unwrap_or(false)
        {
            return true;
        }
        current = dom.node(node).parent;
    }
    false
}

/// Render the main article as a standalone HTML document for reader mode.
pub fn reader_document(dom: &Dom, styles: &[Style]) -> (String, String) {
    let title = dom.title();
    let root = article_root(dom);
    let mut b = Builder {
        out: Vec::new(),
        buf: String::new(),
        heading: None,
    };

    for id in dom.descendants(root) {
        if skipped(dom, styles, id) {
            continue;
        }
        match &dom.node(id).data {
            NodeData::Text(text) => {
                b.buf.push_str(&collapse(text));
            }
            NodeData::Element(el) => {
                match el.tag.as_str() {
                    "br" => b.buf.push('\n'),
                    "li" => {
                        b.flush();
                        b.buf.push_str("\u{2022} ");
                    }
                    "p" | "div" | "section" | "blockquote" | "tr" | "pre" | "figure" | "figcaption" => {
                        b.flush();
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        b.flush();
                        b.heading = el.tag[1..].parse().ok();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Close any skipped subtree state; text after skipped elements is included.
    b.flush();
    let title = if title.trim().is_empty() {
        "Reader".to_string()
    } else {
        title
    };
    let mut html = String::with_capacity(b.out.iter().map(|s| s.len()).sum::<usize>() + 1024);
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\"><title>");
    html.push_str(&escape(&title));
    html.push_str("</title><style>");
    html.push_str(
        "body{margin:0;padding:48px 24px;background:#fff;color:#1a1b1e;font-family:sans-serif;font-size:19px;line-height:1.7}
         .reader{max-width:720px}
         h1{font-size:2em;line-height:1.25;margin:0 0 0.6em}
         h2{font-size:1.4em;margin:1.6em 0 0.4em}
         p{margin:0 0 1.1em}
         figure{margin:1.2em 0}",
    );
    html.push_str("</style></head><body><div class=\"reader\">");
    for block in &b.out {
        html.push_str(block);
    }
    html.push_str("</div></body></html>");
    (title, html)
}

/// Whether the page likely needs JavaScript to show meaningful content.
pub fn is_js_heavy(dom: &Dom, styles: &[Style]) -> bool {
    let mut scripts = 0usize;
    let mut external = 0usize;
    for id in dom.document_order() {
        if let Some(el) = dom.element(id) {
            if el.tag == "script" {
                scripts += 1;
                if el.attr("src").is_some() {
                    external += 1;
                }
            }
        }
    }
    let body = dom.body().unwrap_or(dom.root);
    let text_chars = dom
        .text_content(body)
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    let visible = styles
        .get(body)
        .map(|s| s.display != Display::None)
        .unwrap_or(true);
    !visible || (external >= 2 && text_chars < 400) || (scripts > 0 && text_chars < 120)
}
