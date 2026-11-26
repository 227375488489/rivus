//! A pragmatic CSS subset: tokenising, stylesheet parsing and selector matching.
//!
//! The parser deliberately ignores anything it does not understand instead of
//! failing, which is what a browser has to do anyway when faced with the real web.

use crate::dom::{Dom, NodeId};

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub decls: Vec<Declaration>,
    pub order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Debug, Clone)]
pub struct Selector {
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    pub combinators: Vec<Combinator>,
    pub spec: Spec,
}

#[derive(Debug, Clone, Default)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSelector>,
    pub pseudos: Vec<Pseudo>,
}

#[derive(Debug, Clone)]
pub enum AttrSelector {
    Exists(String),
    Equals(String, String),
    Includes(String, String),
    Prefix(String, String),
    Suffix(String, String),
    Contains(String, String),
}

#[derive(Debug, Clone)]
pub enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    NthChild(i32, i32),
    NthLastChild(i32, i32),
    Not(Box<Compound>),
    Is(Vec<Selector>),
    Where(Vec<Selector>),
    Root,
    Empty,
    Link,
    Checked,
    Disabled,
    Lang(String),
    /// State pseudos that a static renderer can never satisfy.
    Never,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spec {
    pub ids: u32,
    pub classes: u32,
    pub tags: u32,
}

impl Spec {
    pub fn score(&self) -> u32 {
        (self.ids.min(255) << 20) | (self.classes.min(255) << 10) | self.tags.min(255)
    }
}

impl Selector {
    pub fn specificity(&self) -> u32 {
        self.spec.score()
    }
}

// ---------------------------------------------------------------------------
// Stylesheet parsing
// ---------------------------------------------------------------------------

pub fn parse_stylesheet(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut order = 0usize;
    parse_into(text, &mut rules, &mut order);
    rules
}

fn parse_into(text: &str, rules: &mut Vec<Rule>, order: &mut usize) {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' | b';' => i += 1,
            b'@' => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_byte(bytes[i]) {
                    i += 1;
                }
                let name = text[start + 1..i].to_ascii_lowercase();
                let stmt_end = find_any_top(bytes, i, b";{");
                if stmt_end >= bytes.len() {
                    return;
                }
                if bytes[stmt_end] == b'{' {
                    let Some(end) = matching_brace(bytes, stmt_end) else {
                        return;
                    };
                    let prelude = text[i..stmt_end].trim().to_ascii_lowercase();
                    let inner = &text[stmt_end + 1..end];
                    match name.as_str() {
                        "media" => {
                            if !prelude.contains("print") {
                                parse_into(inner, rules, order);
                            }
                        }
                        "supports" | "layer" | "container" => parse_into(inner, rules, order),
                        _ => {}
                    }
                    i = end + 1;
                } else {
                    i = stmt_end + 1;
                }
            }
            _ => {
                let prelude_start = i;
                let brace = find_any_top(bytes, i, b"{");
                if brace >= bytes.len() {
                    return;
                }
                let Some(end) = matching_brace(bytes, brace) else {
                    return;
                };
                let prelude = text[prelude_start..brace].trim();
                let body = &text[brace + 1..end];
                let decls = parse_declarations(body);
                if !decls.is_empty() {
                    let selectors: Vec<Selector> = split_top_level(prelude, ',')
                        .into_iter()
                        .filter_map(|s| parse_selector(s.trim()))
                        .collect();
                    if !selectors.is_empty() {
                        rules.push(Rule {
                            selectors,
                            decls,
                            order: *order,
                        });
                        *order += 1;
                    }
                }
                i = end + 1;
            }
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

fn skip_quoted(bytes: &[u8], mut i: usize) -> usize {
    let quote = bytes[i];
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// Index of the first occurrence of any byte in `needles` outside quotes,
/// parentheses and brackets.
fn find_any_top(bytes: &[u8], mut i: usize, needles: &[u8]) -> usize {
    let mut depth = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' | b'\'' => {
                i = skip_quoted(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ => {
                if depth <= 0 && needles.contains(&b) {
                    return i;
                }
            }
        }
        i += 1;
    }
    bytes.len()
}

fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                i = skip_quoted(bytes, i);
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                i = skip_quoted(bytes, i);
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b if b == sep as u8 && depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

pub fn parse_declarations(text: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for chunk in split_top_level(text, ';') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let Some(colon) = chunk.find(':') else {
            continue;
        };
        let prop = chunk[..colon].trim().to_ascii_lowercase();
        if prop.is_empty() || prop.contains('{') {
            continue;
        }
        let mut value = chunk[colon + 1..].trim().to_string();
        let mut important = false;
        let lower = value.to_ascii_lowercase();
        if let Some(pos) = lower.rfind("!important") {
            if lower[pos..].trim() == "!important" {
                important = true;
                value.truncate(pos);
                value = value.trim().to_string();
            }
        }
        if value.is_empty() {
            continue;
        }
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push(Declaration {
            prop,
            value,
            important,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Selector parsing
// ---------------------------------------------------------------------------

pub fn parse_selector(text: &str) -> Option<Selector> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut compounds: Vec<Compound> = Vec::new();
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut i = 0usize;
    let mut pending: Option<Combinator> = None;
    let mut spec = Spec::default();

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            if !compounds.is_empty() && pending.is_none() {
                pending = Some(Combinator::Descendant);
            }
            i += 1;
            continue;
        }
        if matches!(b, b'>' | b'+' | b'~') {
            pending = Some(match b {
                b'>' => Combinator::Child,
                b'+' => Combinator::Adjacent,
                _ => Combinator::Sibling,
            });
            i += 1;
            continue;
        }

        let (compound, used) = parse_compound(&text[i..])?;
        if used == 0 {
            return None;
        }
        if !compounds.is_empty() {
            combinators.push(pending.unwrap_or(Combinator::Descendant));
        } else if pending == Some(Combinator::Child)
            || pending == Some(Combinator::Adjacent)
            || pending == Some(Combinator::Sibling)
        {
            return None;
        }
        pending = None;
        spec.ids += compound
            .id
            .as_ref()
            .map(|_| 1)
            .unwrap_or(0);
        spec.classes += compound.classes.len() as u32
            + compound.attrs.len() as u32
            + compound
                .pseudos
                .iter()
                .filter(|p| {
                    !matches!(
                        p,
                        Pseudo::Is(_) | Pseudo::Where(_) | Pseudo::Not(_)
                    )
                })
                .count() as u32;
        spec.tags += compound.tag.as_ref().map(|_| 1).unwrap_or(0);
        for pseudo in &compound.pseudos {
            match pseudo {
                Pseudo::Is(list) | Pseudo::Where(list) => {
                    let best = list.iter().map(|s| s.spec).max_by_key(|s| s.score());
                    if let Some(best) = best {
                        if !matches!(pseudo, Pseudo::Where(_)) {
                            spec.ids += best.ids;
                            spec.classes += best.classes;
                            spec.tags += best.tags;
                        }
                    }
                }
                Pseudo::Not(inner) => {
                    let inner_spec = compound_spec(inner);
                    spec.ids += inner_spec.ids;
                    spec.classes += inner_spec.classes;
                    spec.tags += inner_spec.tags;
                }
                _ => {}
            }
        }
        compounds.push(compound);
        i += used;
    }

    if compounds.is_empty() || compounds.len() != combinators.len() + 1 {
        return None;
    }
    Some(Selector {
        compounds,
        combinators,
        spec,
    })
}

fn compound_spec(c: &Compound) -> Spec {
    let mut spec = Spec {
        ids: c.id.as_ref().map(|_| 1).unwrap_or(0),
        classes: c.classes.len() as u32 + c.attrs.len() as u32,
        tags: c.tag.as_ref().map(|_| 1).unwrap_or(0),
    };
    spec.classes += c
        .pseudos
        .iter()
        .filter(|p| !matches!(p, Pseudo::Is(_) | Pseudo::Where(_) | Pseudo::Not(_)))
        .count() as u32;
    spec
}

fn read_ident(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut out = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            out.push(bytes[i + 1] as char);
            i += 2;
        } else if is_ident_byte(b) {
            let ch = s[i..].chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some((out, i))
    }
}

fn parse_compound(s: &str) -> Option<(Compound, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let mut compound = Compound::default();

    while i < bytes.len() {
        match bytes[i] {
            b'*' => {
                i += 1;
            }
            b'.' => {
                let (name, used) = read_ident(&s[i + 1..])?;
                compound.classes.push(name);
                i += 1 + used;
            }
            b'#' => {
                let (name, used) = read_ident(&s[i + 1..])?;
                compound.id = Some(name);
                i += 1 + used;
            }
            b'[' => {
                let end = find_closing(bytes, i, b'[', b']')?;
                let inner = &s[i + 1..end];
                compound.attrs.push(parse_attr_selector(inner));
                i = end + 1;
            }
            b':' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b':' {
                    // Pseudo-elements are not rendered.
                    return None;
                }
                let (name, used) = read_ident(&s[i + 1..])?;
                let mut j = i + 1 + used;
                let name = name.to_ascii_lowercase();
                let mut arg = None;
                if j < bytes.len() && bytes[j] == b'(' {
                    let end = find_closing(bytes, j, b'(', b')')?;
                    arg = Some(s[j + 1..end].to_string());
                    j = end + 1;
                }
                compound.pseudos.push(make_pseudo(&name, arg.as_deref())?);
                i = j;
            }
            b if b.is_ascii_whitespace() || matches!(b, b'>' | b'+' | b'~') => break,
            _ => {
                if compound.tag.is_none() && !is_ident_byte(bytes[i]) {
                    return None;
                }
                if compound.tag.is_none() {
                    let (name, used) = read_ident(&s[i..])?;
                    compound.tag = Some(name.to_ascii_lowercase());
                    i += used;
                } else {
                    break;
                }
            }
        }
    }

    Some((compound, i))
}

fn find_closing(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                i = skip_quoted(bytes, i);
                continue;
            }
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_attr_selector(inner: &str) -> AttrSelector {
    let inner = inner.trim();
    let bytes = inner.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && is_ident_byte(bytes[i]) {
        i += 1;
    }
    let name = inner[..i].to_ascii_lowercase();
    let rest = inner[i..].trim();
    if rest.is_empty() {
        return AttrSelector::Exists(name);
    }
    let (op, rest) = if let Some(r) = rest.strip_prefix("~=") {
        ("~=", r)
    } else if let Some(r) = rest.strip_prefix("|=") {
        ("|=", r)
    } else if let Some(r) = rest.strip_prefix("^=") {
        ("^=", r)
    } else if let Some(r) = rest.strip_prefix("$=") {
        ("$=", r)
    } else if let Some(r) = rest.strip_prefix("*=") {
        ("*=", r)
    } else if let Some(r) = rest.strip_prefix('=') {
        ("=", r)
    } else {
        return AttrSelector::Exists(name);
    };
    let value = rest.trim().trim_matches('"').trim_matches('\'').to_string();
    match op {
        "=" => AttrSelector::Equals(name, value),
        "~=" => AttrSelector::Includes(name, value),
        "^=" => AttrSelector::Prefix(name, value),
        "$=" => AttrSelector::Suffix(name, value),
        _ => AttrSelector::Contains(name, value),
    }
}

fn make_pseudo(name: &str, arg: Option<&str>) -> Option<Pseudo> {
    Some(match name {
        "first-child" => Pseudo::FirstChild,
        "last-child" => Pseudo::LastChild,
        "only-child" => Pseudo::OnlyChild,
        "first-of-type" => Pseudo::FirstOfType,
        "last-of-type" => Pseudo::LastOfType,
        "nth-child" => {
            let (a, b) = parse_nth(arg?)?;
            Pseudo::NthChild(a, b)
        }
        "nth-last-child" => {
            let (a, b) = parse_nth(arg?)?;
            Pseudo::NthLastChild(a, b)
        }
        "not" => Pseudo::Not(Box::new(
            parse_compound(arg?.trim()).map(|(c, _)| c).unwrap_or_default(),
        )),
        "is" | "matches" => Pseudo::Is(
            split_top_level(arg?, ',')
                .into_iter()
                .filter_map(|s| parse_selector(s.trim()))
                .collect(),
        ),
        "where" => Pseudo::Where(
            split_top_level(arg?, ',')
                .into_iter()
                .filter_map(|s| parse_selector(s.trim()))
                .collect(),
        ),
        "root" => Pseudo::Root,
        "empty" => Pseudo::Empty,
        "link" | "any-link" => Pseudo::Link,
        "checked" => Pseudo::Checked,
        "disabled" => Pseudo::Disabled,
        "lang" => Pseudo::Lang(arg.unwrap_or("").trim().to_string()),
        _ => Pseudo::Never,
    })
}

pub fn parse_nth(text: &str) -> Option<(i32, i32)> {
    let s = text.trim().to_ascii_lowercase().replace(' ', "");
    if s.is_empty() {
        return None;
    }
    match s.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if !s.contains('n') {
        return s.parse::<i32>().ok().map(|b| (0, b));
    }
    let (a_part, b_part) = s.split_once('n')?;
    let a = match a_part {
        "" | "+" => 1,
        "-" => -1,
        other => other.parse::<i32>().ok()?,
    };
    let b = if b_part.is_empty() {
        0
    } else {
        b_part
            .strip_prefix('+')
            .unwrap_or(b_part)
            .parse::<i32>()
            .ok()?
    };
    Some((a, b))
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

pub fn matches(dom: &Dom, node: NodeId, selector: &Selector) -> bool {
    match_from(dom, node, selector, selector.compounds.len().saturating_sub(1))
}

fn match_from(dom: &Dom, node: NodeId, selector: &Selector, idx: usize) -> bool {
    if !matches_compound(dom, node, &selector.compounds[idx]) {
        return false;
    }
    if idx == 0 {
        return true;
    }
    match selector.combinators[idx - 1] {
        Combinator::Child => dom
            .parent_element(node)
            .map(|p| match_from(dom, p, selector, idx - 1))
            .unwrap_or(false),
        Combinator::Adjacent => dom
            .previous_element_sibling(node)
            .map(|p| match_from(dom, p, selector, idx - 1))
            .unwrap_or(false),
        Combinator::Sibling => {
            let mut current = dom.previous_element_sibling(node);
            while let Some(prev) = current {
                if match_from(dom, prev, selector, idx - 1) {
                    return true;
                }
                current = dom.previous_element_sibling(prev);
            }
            false
        }
        Combinator::Descendant => {
            let mut current = dom.parent_element(node);
            while let Some(ancestor) = current {
                if match_from(dom, ancestor, selector, idx - 1) {
                    return true;
                }
                current = dom.parent_element(ancestor);
            }
            false
        }
    }
}

pub fn matches_compound(dom: &Dom, node: NodeId, compound: &Compound) -> bool {
    let Some(el) = dom.element(node) else {
        return false;
    };
    if let Some(tag) = &compound.tag {
        if !el.tag.eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(id) = &compound.id {
        if el.id.as_deref() != Some(id.as_str()) {
            return false;
        }
    }
    for class in &compound.classes {
        if !el.has_class(class) {
            return false;
        }
    }
    for attr in &compound.attrs {
        let ok = match attr {
            AttrSelector::Exists(name) => el.attr(name).is_some(),
            AttrSelector::Equals(name, value) => {
                el.attr(name).map(|v| v == value).unwrap_or(false)
            }
            AttrSelector::Includes(name, value) => el
                .attr(name)
                .map(|v| v.split_ascii_whitespace().any(|part| part == value))
                .unwrap_or(false),
            AttrSelector::Prefix(name, value) => {
                el.attr(name).map(|v| v.starts_with(value)).unwrap_or(false)
            }
            AttrSelector::Suffix(name, value) => {
                el.attr(name).map(|v| v.ends_with(value)).unwrap_or(false)
            }
            AttrSelector::Contains(name, value) => {
                el.attr(name).map(|v| v.contains(value)).unwrap_or(false)
            }
        };
        if !ok {
            return false;
        }
    }
    for pseudo in &compound.pseudos {
        if !matches_pseudo(dom, node, pseudo) {
            return false;
        }
    }
    true
}

fn matches_pseudo(dom: &Dom, node: NodeId, pseudo: &Pseudo) -> bool {
    let index = dom.element_index(node);
    let count = dom.element_sibling_count(node);
    let reversed = count.saturating_sub(index + 1);
    match pseudo {
        Pseudo::FirstChild => index == 0,
        Pseudo::LastChild => index + 1 == count,
        Pseudo::OnlyChild => count == 1,
        Pseudo::FirstOfType => {
            let tag = dom.tag(node).unwrap_or("");
            dom.parent_element(node)
                .map(|p| {
                    dom.child_elements(p)
                        .find(|c| dom.tag(*c) == Some(tag))
                        .map(|c| c == node)
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        }
        Pseudo::LastOfType => {
            let tag = dom.tag(node).unwrap_or("");
            dom.parent_element(node)
                .map(|p| {
                    dom.child_elements(p)
                        .filter(|c| dom.tag(*c) == Some(tag))
                        .last()
                        .map(|c| c == node)
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        }
        Pseudo::NthChild(a, b) => nth_matches(*a, *b, index as i32 + 1),
        Pseudo::NthLastChild(a, b) => nth_matches(*a, *b, reversed as i32 + 1),
        Pseudo::Not(inner) => !matches_compound(dom, node, inner),
        Pseudo::Is(list) => list.iter().any(|sel| matches(dom, node, sel)),
        Pseudo::Where(list) => list.iter().any(|sel| matches(dom, node, sel)),
        Pseudo::Root => dom.nodes[node].parent.map(|p| p == 0).unwrap_or(false),
        Pseudo::Empty => dom.nodes[node].children.iter().all(|c| match &dom.nodes[*c].data {
            crate::dom::NodeData::Element(_) => false,
            crate::dom::NodeData::Text(t) => t.trim().is_empty(),
            _ => true,
        }),
        Pseudo::Link => {
            dom.tag(node).map(|t| t == "a" || t == "area").unwrap_or(false)
                && dom.attr(node, "href").is_some()
        }
        Pseudo::Checked => {
            dom.attr(node, "checked").is_some() || dom.attr(node, "selected").is_some()
        }
        Pseudo::Disabled => dom.attr(node, "disabled").is_some(),
        Pseudo::Lang(lang) => lang.is_empty(),
        Pseudo::Never => false,
    }
}

fn nth_matches(a: i32, b: i32, pos: i32) -> bool {
    if a == 0 {
        return pos == b;
    }
    let diff = pos - b;
    diff % a == 0 && diff / a >= 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parse_html;

    #[test]
    fn parses_rules() {
        let rules = parse_stylesheet(
            "/* c */ body { margin: 0; color: #fff } @media screen and (max-width: 100px) { p.x { font-size: 2em !important } } @font-face { font-family: X; src: url(a) }",
        );
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].selectors[0].compounds[0].tag.as_deref(), Some("p"));
        assert!(rules[1].decls[0].important);
    }

    #[test]
    fn matches_complex_selectors() {
        let dom = parse_html(
            b"<body><div id='a' class='wrap'><p class='x'>hi</p><p>no</p></div></body>",
        );
        let p = dom.find_first("p").unwrap();
        let sel = parse_selector("div.wrap > p.x:first-child").unwrap();
        assert!(matches_compound(
            &dom,
            p,
            &sel.compounds[sel.compounds.len() - 1]
        ));
        assert!(matches(&dom, p, &sel));
        let sel2 = parse_selector("#a p:last-child").unwrap();
        let all: Vec<NodeId> = (0..dom.nodes.len())
            .filter(|n| dom.is_element(*n, "p"))
            .collect();
        assert!(matches(&dom, all[1], &sel2));
        assert!(!matches(&dom, all[0], &sel2));
    }

    #[test]
    fn nth_child_math() {
        assert_eq!(parse_nth("2n+1"), Some((2, 1)));
        assert_eq!(parse_nth("odd"), Some((2, 1)));
        assert_eq!(parse_nth("-n+3"), Some((-1, 3)));
        assert!(nth_matches(2, 1, 3));
        assert!(!nth_matches(2, 1, 4));
    }
}
