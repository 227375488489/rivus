//! Computed styles: a compact box-model representation plus the cascade.

use crate::css::{matches, Declaration, Rule};
use crate::dom::{Dom, NodeData, NodeId};

pub type Rgba = [u8; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    ListItem,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Sans,
    Serif,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    Nowrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStyle {
    Disc,
    Circle,
    Square,
    Decimal,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Factor(f32),
    Px(f32),
}

impl LineHeight {
    pub fn resolve(self, font_size: f32) -> f32 {
        match self {
            LineHeight::Normal => (font_size * 1.45).max(font_size + 2.0),
            LineHeight::Factor(f) => (font_size * f.clamp(0.5, 4.0)).max(1.0),
            LineHeight::Px(px) => px.max(1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Edges {
    pub fn all(v: f32) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn is_zero(&self) -> bool {
        self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0 && self.left == 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Len {
    Px(f32),
    Pct(f32),
    Em(f32),
    Auto,
}

impl Len {
    /// Resolve to pixels. `base` is the containing-block width for percentages,
    /// `em` the current font size for em units.
    pub fn resolve(&self, base: f32, em: f32) -> Option<f32> {
        match self {
            Len::Px(v) => Some(*v),
            Len::Pct(v) => Some(base * v / 100.0),
            Len::Em(v) => Some(em * v),
            Len::Auto => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Style {
    pub display: Display,
    pub color: Rgba,
    pub background: Option<Rgba>,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub family: FontFamily,
    pub line_height: LineHeight,
    pub align: Align,
    pub underline: bool,
    pub strike: bool,
    pub white_space: WhiteSpace,
    pub uppercase: bool,
    pub letter_spacing: f32,
    pub margin: Edges,
    pub padding: Edges,
    pub border: Edges,
    pub border_color: Rgba,
    pub radius: f32,
    pub width: Option<Len>,
    pub max_width: Option<Len>,
    pub min_width: Option<Len>,
    pub height: Option<Len>,
    pub opacity: f32,
    pub visible: bool,
    pub list_style: ListStyle,
    pub is_link: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            color: [26, 27, 30, 255],
            background: None,
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            family: FontFamily::Sans,
            line_height: LineHeight::Normal,
            align: Align::Left,
            underline: false,
            strike: false,
            white_space: WhiteSpace::Normal,
            uppercase: false,
            letter_spacing: 0.0,
            margin: Edges::default(),
            padding: Edges::default(),
            border: Edges::default(),
            border_color: [200, 200, 200, 255],
            radius: 0.0,
            width: None,
            max_width: None,
            min_width: None,
            height: None,
            opacity: 1.0,
            visible: true,
            list_style: ListStyle::Disc,
            is_link: false,
        }
    }
}

impl Style {
    fn inherit_from(parent: &Style) -> Self {
        let mut style = Style::default();
        style.color = parent.color;
        style.font_size = parent.font_size;
        style.font_weight = parent.font_weight;
        style.italic = parent.italic;
        style.family = parent.family;
        style.line_height = parent.line_height;
        style.align = parent.align;
        style.white_space = parent.white_space;
        style.letter_spacing = parent.letter_spacing;
        style.list_style = parent.list_style;
        style
    }

    pub fn line_px(&self) -> f32 {
        self.line_height.resolve(self.font_size)
    }

    pub fn is_block_level(&self) -> bool {
        matches!(self.display, Display::Block | Display::ListItem)
    }
}

// ---------------------------------------------------------------------------
// Colour parsing
// ---------------------------------------------------------------------------

pub fn parse_color(value: &str) -> Option<Rgba> {
    let v = value.trim().to_ascii_lowercase();
    if let Some(hex) = v.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = v.strip_prefix("rgb(").or_else(|| v.strip_prefix("rgba(")) {
        let inner = inner.trim_end_matches(')').replace('/', ",");
        let parts: Vec<&str> = inner
            .split([',', ' '])
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 3 {
            let r = parse_channel(parts[0])?;
            let g = parse_channel(parts[1])?;
            let b = parse_channel(parts[2])?;
            let a = parts
                .get(3)
                .and_then(|p| parse_alpha(p))
                .unwrap_or(255);
            return Some([r, g, b, a]);
        }
        return None;
    }
    if let Some(inner) = v.strip_prefix("hsl(").or_else(|| v.strip_prefix("hsla(")) {
        let inner = inner.trim_end_matches(')').replace('/', ",");
        let parts: Vec<&str> = inner
            .split([',', ' '])
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 3 {
            let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?;
            let s = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let l = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
            let a = parts.get(3).and_then(|p| parse_alpha(p)).unwrap_or(255);
            return Some(hsl_to_rgba(h, s, l, a));
        }
        return None;
    }
    named_color(&v)
}

fn parse_hex_color(hex: &str) -> Option<Rgba> {
    let chars: Vec<char> = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    match chars.len() {
        3 | 4 => {
            let mut out = [0u8; 4];
            out[3] = 255;
            for (i, c) in chars.iter().enumerate() {
                let v = c.to_digit(16)? as u8;
                out[i] = v * 17;
            }
            Some(out)
        }
        6 | 8 => {
            let mut out = [0u8; 4];
            out[3] = 255;
            for i in 0..chars.len() / 2 {
                let hi = chars[i * 2].to_digit(16)? as u8;
                let lo = chars[i * 2 + 1].to_digit(16)? as u8;
                out[i] = hi * 16 + lo;
            }
            Some(out)
        }
        _ => None,
    }
}

fn parse_channel(part: &str) -> Option<u8> {
    if let Some(p) = part.strip_suffix('%') {
        let v = p.parse::<f32>().ok()?;
        Some(((v / 100.0) * 255.0).clamp(0.0, 255.0) as u8)
    } else {
        let v = part.parse::<f32>().ok()?;
        Some(v.clamp(0.0, 255.0) as u8)
    }
}

fn parse_alpha(part: &str) -> Option<u8> {
    if let Some(p) = part.strip_suffix('%') {
        let v = p.parse::<f32>().ok()?;
        Some(((v / 100.0) * 255.0).clamp(0.0, 255.0) as u8)
    } else {
        let v = part.parse::<f32>().ok()?;
        let scaled = if v <= 1.0 { v * 255.0 } else { v };
        Some(scaled.clamp(0.0, 255.0) as u8)
    }
}

fn hsl_to_rgba(h: f32, s: f32, l: f32, a: u8) -> Rgba {
    let h = (h.rem_euclid(360.0)) / 360.0;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return [v, v, v, a];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |mut t: f32| -> u8 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0) as u8
    };
    [
        channel(h + 1.0 / 3.0),
        channel(h),
        channel(h - 1.0 / 3.0),
        a,
    ]
}

fn named_color(name: &str) -> Option<Rgba> {
    Some(match name {
        "transparent" => [0, 0, 0, 0],
        "currentcolor" => return None,
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "cyan" | "aqua" => [0, 255, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "silver" => [192, 192, 192, 255],
        "maroon" => [128, 0, 0, 255],
        "olive" => [128, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "teal" => [0, 128, 128, 255],
        "navy" => [0, 0, 128, 255],
        "purple" => [128, 0, 128, 255],
        "orange" => [255, 165, 0, 255],
        "pink" => [255, 192, 203, 255],
        "brown" => [165, 42, 42, 255],
        "gold" => [255, 215, 0, 255],
        "indigo" => [75, 0, 130, 255],
        "violet" => [238, 130, 238, 255],
        "crimson" => [220, 20, 60, 255],
        "salmon" => [250, 128, 114, 255],
        "tomato" => [255, 99, 71, 255],
        "coral" => [255, 127, 80, 255],
        "khaki" => [240, 230, 140, 255],
        "beige" => [245, 245, 220, 255],
        "ivory" => [255, 255, 240, 255],
        "lavender" => [230, 230, 250, 255],
        "plum" => [221, 160, 221, 255],
        "turquoise" => [64, 224, 208, 255],
        "skyblue" => [135, 206, 235, 255],
        "steelblue" => [70, 130, 180, 255],
        "royalblue" => [65, 105, 225, 255],
        "midnightblue" => [25, 25, 112, 255],
        "forestgreen" => [34, 139, 34, 255],
        "seagreen" => [46, 139, 87, 255],
        "darkgreen" => [0, 100, 0, 255],
        "darkred" => [139, 0, 0, 255],
        "darkblue" => [0, 0, 139, 255],
        "darkgray" | "darkgrey" => [169, 169, 169, 255],
        "lightgray" | "lightgrey" => [211, 211, 211, 255],
        "whitesmoke" => [245, 245, 245, 255],
        "gainsboro" => [220, 220, 220, 255],
        "snow" => [255, 250, 250, 255],
        "seashell" => [255, 245, 238, 255],
        "linen" => [250, 240, 230, 255],
        "oldlace" => [253, 245, 230, 255],
        "cornsilk" => [255, 248, 220, 255],
        "lemonchiffon" => [255, 250, 205, 255],
        "azure" => [240, 255, 255, 255],
        "aliceblue" => [240, 248, 255, 255],
        "ghostwhite" => [248, 248, 255, 255],
        "mintcream" => [245, 255, 250, 255],
        "honeydew" => [240, 255, 240, 255],
        "rebeccapurple" => [102, 51, 153, 255],
        "dimgray" | "dimgrey" => [105, 105, 105, 255],
        "slategray" | "slategrey" => [112, 128, 144, 255],
        "firebrick" => [178, 34, 34, 255],
        "chocolate" => [210, 105, 30, 255],
        "peru" => [205, 133, 63, 255],
        "tan" => [210, 180, 140, 255],
        "wheat" => [245, 222, 179, 255],
        "orchid" => [218, 112, 214, 255],
        "hotpink" => [255, 105, 180, 255],
        "deeppink" => [255, 20, 147, 255],
        "cadeblue" | "cadetblue" => [95, 158, 160, 255],
        "darkorange" => [255, 140, 0, 255],
        "goldenrod" => [218, 165, 32, 255],
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Lengths and keywords
// ---------------------------------------------------------------------------

pub fn parse_len(value: &str) -> Option<Len> {
    let v = value.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(Len::Auto);
    }
    if v == "0" {
        return Some(Len::Px(0.0));
    }
    let (num, unit) = split_unit(&v)?;
    let n: f32 = num.parse().ok()?;
    Some(match unit {
        "" => Len::Px(n),
        "px" => Len::Px(n),
        "pt" => Len::Px(n * 4.0 / 3.0),
        "pc" => Len::Px(n * 16.0),
        "in" => Len::Px(n * 96.0),
        "cm" => Len::Px(n * 37.795),
        "mm" => Len::Px(n * 3.7795),
        "q" => Len::Px(n * 0.9449),
        "em" => Len::Em(n),
        "rem" => Len::Em(n),
        "%" => Len::Pct(n),
        "vw" => Len::Pct(n / 100.0 * 100.0),
        "vh" => Len::Px(n * 8.0),
        _ => return None,
    })
}

fn split_unit(v: &str) -> Option<(&str, &str)> {
    let bytes = v.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut seen_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        seen_digit = true;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            seen_digit = true;
            i += 1;
        }
    }
    if !seen_digit {
        return None;
    }
    // Optional scientific exponent: e[+-]?digits
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let digits_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits_start {
            i = j;
        }
    }
    Some((&v[..i], &v[i..]))
}

fn parse_edges(values: &[&str], base: f32) -> Option<Edges> {
    let values: Vec<&str> = values.iter().copied().filter(|v| !v.is_empty()).collect();
    if values.is_empty() {
        return None;
    }
    let len = |s: &str| parse_len(s).and_then(|l| l.resolve(base, base));
    let (top, right, bottom, left) = match values.len() {
        1 => {
            let v = len(values[0])?;
            (v, v, v, v)
        }
        2 => {
            let v = len(values[0])?;
            let h = len(values[1])?;
            (v, h, v, h)
        }
        3 => {
            let t = len(values[0])?;
            let h = len(values[1])?;
            let b = len(values[2])?;
            (t, h, b, h)
        }
        _ => (
            len(values[0])?,
            len(values[1])?,
            len(values[2])?,
            len(values[3])?,
        ),
    };
    Some(Edges {
        top,
        right,
        bottom,
        left,
    })
}

pub fn parse_border_width(value: &str) -> f32 {
    let v = value.trim().to_ascii_lowercase();
    match v.as_str() {
        "thin" => 1.0,
        "medium" => 3.0,
        "thick" => 5.0,
        _ => parse_len(&v)
            .and_then(|l| l.resolve(0.0, 16.0))
            .unwrap_or(0.0)
            .max(0.0),
    }
}

// ---------------------------------------------------------------------------
// Cascade
// ---------------------------------------------------------------------------

pub fn compute_styles(dom: &Dom, rules: &[Rule]) -> Vec<Style> {
    let count = dom.nodes.len();
    let mut styles = vec![Style::default(); count];
    styles[0].display = Display::Block;
    let order = dom.document_order();
    for id in order {
        if id == 0 {
            continue;
        }
        let parent = dom.nodes[id].parent.unwrap_or(0);
        match &dom.nodes[id].data {
            NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype => {
                styles[id] = styles[parent].clone();
                styles[id].display = Display::Inline;
                styles[id].background = None;
                styles[id].margin = Edges::default();
                styles[id].padding = Edges::default();
                styles[id].border = Edges::default();
            }
            NodeData::Document => {
                styles[id].display = Display::Block;
            }
            NodeData::Element(el) => {
                styles[id] = compute_element(dom, id, rules, &styles[parent]);
                if el.attr("hidden").is_some() {
                    styles[id].display = Display::None;
                }
                // Visually-hidden utilities (skip links, screen-reader labels) are
                // still in the document flow on sites whose CSS we cannot parse.
                // They must never become a vertical column of single letters.
                if let Some(class) = el.attr("class") {
                    let lower = class.to_ascii_lowercase();
                    let hidden = [
                        "sr-only",
                        "visually-hidden",
                        "visuallyhidden",
                        "screen-reader-text",
                        "screen-reader-only",
                        "skip-link",
                        "skip-to-content",
                        "mw-jump-link",
                        "element-invisible",
                    ];
                    if lower
                        .split_whitespace()
                        .any(|token| hidden.contains(&token))
                    {
                        styles[id].display = Display::None;
                    }
                }
            }
        }
    }
    styles
}

fn compute_element(dom: &Dom, id: NodeId, rules: &[Rule], parent: &Style) -> Style {
    let el = dom.element(id).expect("element node");
    let mut style = Style::inherit_from(parent);
    // Only root-level boxes start from the initial font size.
    ua_defaults(&mut style, &el.tag, parent.font_size);
    if el.tag == "a" && el.attr("href").is_some() {
        style.is_link = true;
    }

    let mut matched: Vec<(bool, u32, usize)> = Vec::new();
    let mut decl_sets: Vec<&[Declaration]> = Vec::new();
    for rule in rules {
        let mut best: Option<u32> = None;
        for sel in &rule.selectors {
            if matches(dom, id, sel) {
                let score = sel.specificity();
                best = Some(best.map_or(score, |b: u32| b.max(score)));
            }
        }
        if let Some(score) = best {
            matched.push((false, score, rule.order));
            decl_sets.push(&rule.decls);
        }
    }

    let inline: Vec<Declaration> = el
        .attr("style")
        .map(crate::css::parse_declarations)
        .unwrap_or_default();
    if !inline.is_empty() {
        matched.push((false, u32::MAX - 1, usize::MAX));
        decl_sets.push(&inline);
    }

    let mut order: Vec<usize> = (0..matched.len()).collect();
    order.sort_by_key(|&i| {
        let (important, score, rule_order) = matched[i];
        (important, score, rule_order)
    });

    for decl in order.iter().flat_map(|&i| decl_sets[i].iter()) {
        apply_declaration(&mut style, decl, parent.font_size);
    }

    style
}

const FONT_SIZE_KEYWORDS: &[(&str, f32)] = &[
    ("xx-small", 9.0),
    ("x-small", 10.0),
    ("small", 13.0),
    ("medium", 16.0),
    ("large", 18.0),
    ("x-large", 24.0),
    ("xx-large", 32.0),
];

pub fn apply_declaration(style: &mut Style, decl: &Declaration, parent_font: f32) {
    let prop = decl.prop.as_str();
    let value = decl.value.as_str();
    let lower = value.to_ascii_lowercase();
    let em = style.font_size;

    match prop {
        "display" => {
            style.display = match lower.as_str() {
                "none" => Display::None,
                "block" | "flex" | "grid" | "table" | "table-row" | "table-cell"
                | "table-row-group" | "flow-root" | "contents" => Display::Block,
                "list-item" => Display::ListItem,
                _ => Display::Inline,
            };
        }
        "visibility" => {
            if lower.starts_with("hidden") || lower.starts_with("collapse") {
                style.visible = false;
            } else if lower.starts_with("visible") {
                style.visible = true;
            }
        }
        "color" => {
            if let Some(c) = parse_color(value) {
                style.color = c;
            }
        }
        "background-color" => style.background = parse_color(value),
        "background" => {
            if lower.contains("url(") && !lower.contains("rgb") && !lower.contains('#') {
                // Background images are not fetched: privacy-preserving by default.
            } else if let Some(c) = find_color_token(value) {
                style.background = Some(c);
            }
        }
        "font-size" => {
            let size = if let Some((_, px)) =
                FONT_SIZE_KEYWORDS.iter().find(|(k, _)| *k == lower)
            {
                Some(*px)
            } else if lower == "smaller" {
                Some(parent_font * 0.83)
            } else if lower == "larger" {
                Some(parent_font * 1.2)
            } else {
                parse_len(value).and_then(|l| l.resolve(parent_font, parent_font))
            };
            if let Some(size) = size {
                style.font_size = size.clamp(4.0, 300.0);
            }
        }
        "font-weight" => {
            style.font_weight = match lower.as_str() {
                "normal" => 400,
                "bold" | "bolder" => 700,
                "lighter" => 300,
                _ => lower.parse::<u16>().unwrap_or(400).clamp(100, 900),
            };
        }
        "font-style" => style.italic = lower.starts_with("italic") || lower.starts_with("oblique"),
        "font-family" => {
            style.family = if lower.contains("mono") || lower.contains("courier") {
                FontFamily::Mono
            } else if lower.contains("serif")
                && !lower.contains("sans-serif")
                && !lower.contains("sans serif")
            {
                FontFamily::Serif
            } else {
                FontFamily::Sans
            };
        }
        "line-height" => {
            style.line_height = if lower == "normal" {
                LineHeight::Normal
            } else if let Some(p) = lower.strip_suffix('%') {
                p.parse::<f32>()
                    .map(|v| LineHeight::Factor(v / 100.0))
                    .unwrap_or(LineHeight::Normal)
            } else if let Ok(n) = lower.parse::<f32>() {
                LineHeight::Factor(n)
            } else if let Some(len) = parse_len(value) {
                match len {
                    Len::Px(px) => LineHeight::Px(px),
                    Len::Em(v) => LineHeight::Factor(v),
                    Len::Pct(v) => LineHeight::Factor(v / 100.0),
                    Len::Auto => LineHeight::Normal,
                }
            } else {
                style.line_height
            };
        }
        "text-align" => {
            style.align = match lower.as_str() {
                "center" => Align::Center,
                "right" | "end" => Align::Right,
                "justify" => Align::Justify,
                _ => Align::Left,
            };
        }
        "text-decoration" | "text-decoration-line" => {
            style.underline = lower.contains("underline");
            style.strike = lower.contains("line-through");
        }
        "text-transform" => style.uppercase = lower == "uppercase",
        "letter-spacing" => {
            style.letter_spacing = if lower == "normal" {
                0.0
            } else {
                parse_len(value)
                    .and_then(|l| l.resolve(0.0, em))
                    .unwrap_or(0.0)
            };
        }
        "white-space" => {
            style.white_space = match lower.as_str() {
                "pre" | "pre-wrap" | "break-spaces" => WhiteSpace::Pre,
                "nowrap" | "pre-line" => WhiteSpace::Nowrap,
                _ => WhiteSpace::Normal,
            };
        }
        "margin" => {
            let parts: Vec<&str> = value.split_whitespace().collect();
            if let Some(edges) = parse_edges(&parts, em) {
                style.margin = edges;
            }
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            if let Some(v) = parse_len(value).and_then(|l| l.resolve(em, em)) {
                match prop {
                    "margin-top" => style.margin.top = v,
                    "margin-right" => style.margin.right = v,
                    "margin-bottom" => style.margin.bottom = v,
                    _ => style.margin.left = v,
                }
            }
        }
        "padding" => {
            let parts: Vec<&str> = value.split_whitespace().collect();
            if let Some(edges) = parse_edges(&parts, em) {
                style.padding = edges;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            if let Some(v) = parse_len(value).and_then(|l| l.resolve(em, em)) {
                match prop {
                    "padding-top" => style.padding.top = v,
                    "padding-right" => style.padding.right = v,
                    "padding-bottom" => style.padding.bottom = v,
                    _ => style.padding.left = v,
                }
            }
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            if lower.contains("none") || lower.contains("hidden") {
                if prop == "border" {
                    style.border = Edges::default();
                }
                return;
            }
            let width = value
                .split_whitespace()
                .find(|tok| {
                    let t = tok.to_ascii_lowercase();
                    t == "thin" || t == "medium" || t == "thick" || split_unit(&t).is_some()
                })
                .map(parse_border_width)
                .unwrap_or(1.0);
            if let Some(c) = find_color_token(value) {
                style.border_color = c;
            }
            match prop {
                "border" => style.border = Edges::all(width),
                "border-top" => style.border.top = width,
                "border-right" => style.border.right = width,
                "border-bottom" => style.border.bottom = width,
                _ => style.border.left = width,
            }
        }
        "border-width" => {
            let parts: Vec<&str> = value.split_whitespace().collect();
            if let Some(edges) = parse_edges(&parts, em) {
                style.border = edges;
            }
        }
        "border-color" => {
            if let Some(c) = parse_color(value) {
                style.border_color = c;
            }
        }
        "border-radius" => {
            if let Some(v) = value
                .split_whitespace()
                .next()
                .and_then(|v| parse_len(v))
                .and_then(|l| l.resolve(em, em))
            {
                style.radius = v.max(0.0);
            }
        }
        "width" | "max-width" | "min-width" | "height" => {
            let len = if lower == "none" || lower == "auto" {
                Some(Len::Auto)
            } else if lower.ends_with("fit-content") || lower.ends_with("max-content") {
                None
            } else {
                parse_len(value)
            };
            if let Some(len) = len {
                match prop {
                    "width" => style.width = Some(len),
                    "max-width" => style.max_width = Some(len),
                    "min-width" => style.min_width = Some(len),
                    _ => style.height = Some(len),
                }
            }
        }
        "opacity" => {
            if let Ok(v) = lower.parse::<f32>() {
                style.opacity = v.clamp(0.0, 1.0);
            }
        }
        "list-style-type" => {
            style.list_style = match lower.as_str() {
                "none" => ListStyle::None,
                "decimal" | "decimal-leading-zero" | "lower-alpha" | "upper-alpha"
                | "lower-roman" | "upper-roman" => ListStyle::Decimal,
                "circle" => ListStyle::Circle,
                "square" => ListStyle::Square,
                _ => ListStyle::Disc,
            };
        }
        "font" => {
            for (_, px) in FONT_SIZE_KEYWORDS {
                if lower.contains(&format!("{}px", *px as i32)) {
                    style.font_size = *px;
                }
            }
            if lower.contains("bold") {
                style.font_weight = 700;
            }
            if lower.contains("italic") {
                style.italic = true;
            }
            if lower.contains("mono") {
                style.family = FontFamily::Mono;
            }
        }
        _ => {}
    }
}

fn find_color_token(value: &str) -> Option<Rgba> {
    let lower = value.to_ascii_lowercase();
    for token in lower.split([' ', ',']) {
        if token.is_empty() {
            continue;
        }
        if token.starts_with('#')
            || token.starts_with("rgb(")
            || token.starts_with("rgba(")
            || token.starts_with("hsl(")
        {
            if let Some(c) = parse_color(token) {
                return Some(c);
            }
        } else if let Some(c) = named_color(token) {
            return Some(c);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// User-agent defaults
// ---------------------------------------------------------------------------

fn ua_defaults(style: &mut Style, tag: &str, parent_font: f32) {
    match tag {
        "html" => {
            style.display = Display::Block;
        }
        "body" => {
            style.display = Display::Block;
            style.margin = Edges::all(8.0);
            style.font_size = 16.0;
        }
        "head" | "script" | "style" | "meta" | "link" | "base" | "title" | "noscript"
        | "template" | "iframe" | "object" | "embed" | "canvas" | "svg" => {
            style.display = Display::None;
        }
        "div" | "section" | "article" | "main" | "header" | "footer" | "nav"
        | "aside" | "figcaption" | "address"
        | "details" | "fieldset" | "form" | "dl" | "dt" | "dd" | "hgroup"
        | "center" | "caption" | "legend" | "marquee" | "search" => {
            style.display = Display::Block;
        }
        "p" => style.margin = Edges::all(style.font_size * 1.0),
        "blockquote" | "figure" => {
            style.display = Display::Block;
            style.margin = Edges {
                top: style.font_size,
                right: style.font_size * 2.5,
                bottom: style.font_size,
                left: style.font_size * 2.5,
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let (scale, top, bottom) = match tag {
                "h1" => (2.0, 0.67, 0.67),
                "h2" => (1.5, 0.83, 0.83),
                "h3" => (1.17, 1.0, 1.0),
                "h4" => (1.0, 1.33, 1.33),
                "h5" => (0.83, 1.67, 1.67),
                _ => (0.67, 2.33, 2.33),
            };
            style.display = Display::Block;
            style.font_size = (parent_font * scale).max(10.0);
            style.font_weight = 700;
            style.margin = Edges {
                top: parent_font * top,
                right: 0.0,
                bottom: parent_font * bottom,
                left: 0.0,
            };
        }
        "ul" | "ol" => {
            style.display = Display::Block;
            style.margin = Edges::all(style.font_size);
            style.padding.left = style.font_size * 2.5;
            style.list_style = if tag == "ol" {
                ListStyle::Decimal
            } else {
                ListStyle::Disc
            };
        }
        "li" => {
            style.display = Display::ListItem;
            style.margin.top = style.font_size * 0.25;
            style.margin.bottom = style.font_size * 0.25;
        }
        "a" => {
            style.color = [12, 74, 190, 255];
            style.underline = true;
        }
        "strong" | "b" => style.font_weight = 700,
        "em" | "i" | "cite" | "var" | "dfn" => style.italic = true,
        "u" | "ins" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "small" => style.font_size = (style.font_size * 0.85).max(6.0),
        "big" => style.font_size = (style.font_size * 1.2).min(200.0),
        "sub" | "sup" => style.font_size = (style.font_size * 0.75).max(6.0),
        "code" | "kbd" | "samp" | "tt" => {
            style.family = FontFamily::Mono;
            if tag != "pre" {
                style.font_size = (style.font_size * 0.94).max(6.0);
            }
        }
        "pre" => {
            style.display = Display::Block;
            style.white_space = WhiteSpace::Pre;
            style.margin = Edges::all(style.font_size);
            style.font_size = (style.font_size * 0.9).max(6.0);
            style.padding = Edges::all(style.font_size * 0.75);
            style.background = Some([245, 246, 248, 255]);
            style.radius = 6.0;
        }
        "hr" => {
            style.display = Display::Block;
            style.border.top = 1.0;
            style.border_color = [210, 214, 220, 255];
            style.margin = Edges {
                top: style.font_size * 0.5,
                right: 0.0,
                bottom: style.font_size * 0.5,
                left: 0.0,
            };
        }
        "img" => {
            style.display = Display::Inline;
        }
        "table" => {
            style.display = Display::Block;
            style.margin.top = style.font_size * 0.5;
            style.margin.bottom = style.font_size * 0.5;
        }
        "tr" | "thead" | "tbody" | "tfoot" => style.display = Display::Block,
        "td" | "th" => {
            style.display = Display::Block;
            style.padding = Edges {
                top: 6.0,
                right: 10.0,
                bottom: 6.0,
                left: 10.0,
            };
            style.border.bottom = 1.0;
            style.border_color = [230, 232, 236, 255];
            if tag == "th" {
                style.font_weight = 700;
            }
        }
        "mark" => style.background = Some([255, 243, 163, 255]),
        "option" | "optgroup" | "datalist" => style.display = Display::None,
        "button" | "input" | "select" | "textarea" => {
            style.display = Display::Inline;
            style.margin.right = 2.0;
        }
        "summary" => style.font_weight = 700,
        "q" => style.italic = true,
        "abbr" => style.underline = true,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::parse_stylesheet;
    use crate::dom::parse_html;

    #[test]
    fn colours_and_lengths() {
        assert_eq!(parse_color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_color("rgba(0, 0, 0, .5)"), Some([0, 0, 0, 127]));
        assert_eq!(parse_color("steelblue"), Some([70, 130, 180, 255]));
        assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some([255, 0, 0, 255]));
        assert_eq!(parse_len("1.5em"), Some(Len::Em(1.5)));
        assert_eq!(parse_len("12pt"), Some(Len::Px(16.0)));
    }

    #[test]
    fn cascades_rules() {
        let dom = parse_html(
            b"<body><p class='a'>x</p><p style='color: rgb(1,2,3)'>y</p></body>",
        );
        let rules = parse_stylesheet("p { color: red; margin: 10px } p.a { color: #00ff00 }");
        let styles = compute_styles(&dom, &rules);
        let ps: Vec<usize> = (0..dom.nodes.len())
            .filter(|n| dom.is_element(*n, "p"))
            .collect();
        assert_eq!(styles[ps[0]].color, [0, 255, 0, 255]);
        assert_eq!(styles[ps[0]].margin.top, 10.0);
        assert_eq!(styles[ps[1]].color, [1, 2, 3, 255]);
    }

    #[test]
    fn headings_inherit_scale() {
        let dom = parse_html(b"<body><h1>T</h1></body>");
        let styles = compute_styles(&dom, &[]);
        let h1 = dom.find_first("h1").unwrap();
        assert_eq!(styles[h1].font_size, 32.0);
        assert_eq!(styles[h1].font_weight, 700);
    }
}
