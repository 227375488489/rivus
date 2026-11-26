//! A small arena DOM built on top of html5ever's parser.
//!
//! Everything is addressed by `NodeId` (an index into `Dom::nodes`), which keeps
//! tree walking allocation-free and lets the UI hold onto nodes across frames.

use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, ParseOpts};
use markup5ever_rcdom::{NodeData as RcData, RcDom};

pub type NodeId = usize;

#[derive(Debug, Clone, Default)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub classes: Vec<String>,
    pub id: Option<String>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    Doctype,
    Text(String),
    Comment(String),
    Element(Element),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

#[derive(Debug, Clone, Default)]
pub struct Dom {
    pub nodes: Vec<Node>,
    pub root: NodeId,
}

impl Dom {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes[id].data {
            NodeData::Element(el) => Some(el),
            _ => None,
        }
    }

    pub fn tag(&self, id: NodeId) -> Option<&str> {
        self.element(id).map(|el| el.tag.as_str())
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.element(id).and_then(|el| el.attr(name))
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].data {
            NodeData::Text(t) => Some(t.as_str()),
            _ => None,
        }
    }

    pub fn is_element(&self, id: NodeId, tag: &str) -> bool {
        self.element(id)
            .map(|el| el.tag.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    }

    pub fn find_first(&self, tag: &str) -> Option<NodeId> {
        (0..self.nodes.len()).find(|&id| self.is_element(id, tag))
    }

    pub fn find_first_with(&self, tag: &str, attr: &str) -> Option<NodeId> {
        (0..self.nodes.len()).find(|&id| {
            self.is_element(id, tag) && self.attr(id, attr).is_some()
        })
    }

    pub fn body(&self) -> Option<NodeId> {
        self.find_first("body")
    }

    pub fn document_order(&self) -> Vec<NodeId> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            out.push(id);
            for child in self.nodes[id].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }

    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            match &self.nodes[current].data {
                NodeData::Text(t) => out.push_str(t),
                NodeData::Element(el) if el.tag == "br" => out.push('\n'),
                _ => {}
            }
            for child in self.nodes[current].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }

    /// All descendants of `id` in document order, excluding `id`.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(current) = stack.pop() {
            out.push(current);
            for child in self.nodes[current].children.iter().rev() {
                stack.push(*child);
            }
        }
        out
    }

    pub fn child_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|c| self.element(*c).is_some())
    }

    /// Index of `id` among its element siblings, ignoring text nodes.
    pub fn element_index(&self, id: NodeId) -> usize {
        let Some(parent) = self.nodes[id].parent else {
            return 0;
        };
        self.nodes[parent]
            .children
            .iter()
            .filter(|c| self.element(**c).is_some())
            .position(|c| *c == id)
            .unwrap_or(0)
    }

    pub fn element_sibling_count(&self, id: NodeId) -> usize {
        let Some(parent) = self.nodes[id].parent else {
            return 1;
        };
        self.nodes[parent]
            .children
            .iter()
            .filter(|c| self.element(**c).is_some())
            .count()
    }

    pub fn previous_element_sibling(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[id].parent?;
        let mut prev = None;
        for child in &self.nodes[parent].children {
            if *child == id {
                return prev;
            }
            if self.element(*child).is_some() {
                prev = Some(*child);
            }
        }
        None
    }

    /// The nearest ancestor element, skipping the document node.
    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[id].parent?;
        self.element(parent).map(|_| parent)
    }

    pub fn title(&self) -> String {
        self.find_first("title")
            .map(|id| self.text_content(id).trim().to_string())
            .unwrap_or_default()
    }
}

pub fn parse_html(bytes: &[u8]) -> Dom {
    let mut cursor = bytes;
    let rc = match parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut cursor)
    {
        Ok(dom) => dom,
        Err(_) => return Dom::default(),
    };

    let mut dom = Dom {
        nodes: Vec::new(),
        root: 0,
    };
    dom.nodes.push(Node {
        parent: None,
        children: Vec::new(),
        data: NodeData::Document,
    });

    // The RcDom document handle is mapped onto node 0 directly.
    let mut work: Vec<(markup5ever_rcdom::Handle, NodeId)> = rc
        .document
        .children
        .borrow()
        .iter()
        .rev()
        .map(|child| (child.clone(), 0))
        .collect();
    while let Some((handle, parent)) = work.pop() {
        let (kind, children): (NodeData, Vec<markup5ever_rcdom::Handle>) = {
            let data = &handle.data;
            match data {
                RcData::Document => (NodeData::Document, handle.children.borrow().clone()),
                RcData::Doctype { .. } => (NodeData::Doctype, Vec::new()),
                RcData::Text { contents } => {
                    (NodeData::Text(contents.borrow().to_string()), Vec::new())
                }
                RcData::Comment { contents } => {
                    (NodeData::Comment(contents.to_string()), Vec::new())
                }
                RcData::Element { name, attrs, .. } => {
                    let tag = name.local.to_string().to_ascii_lowercase();
                    let attrs: Vec<(String, String)> = attrs
                        .borrow()
                        .iter()
                        .map(|a| {
                            (
                                a.name.local.to_string().to_ascii_lowercase(),
                                a.value.to_string(),
                            )
                        })
                        .collect();
                    let classes = attrs
                        .iter()
                        .find(|(k, _)| k == "class")
                        .map(|(_, v)| {
                            v.split_ascii_whitespace()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let id = attrs
                        .iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.clone());
                    (
                        NodeData::Element(Element {
                            tag,
                            attrs,
                            classes,
                            id,
                        }),
                        handle.children.borrow().clone(),
                    )
                }
                _ => (NodeData::Comment(String::new()), Vec::new()),
            }
        };

        let id = dom.nodes.len();
        dom.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            data: kind,
        });
        dom.nodes[parent].children.push(id);

        for child in children.into_iter().rev() {
            work.push((child, id));
        }
    }

    dom.root = 0;
    dom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_indexes() {
        let dom = parse_html(
            b"<html><head><title>Hi</title></head><body><p class='x lead'>One <b>two</b></p></body></html>",
        );
        assert_eq!(dom.title(), "Hi");
        let p = dom.find_first("p").unwrap();
        assert!(dom.element(p).unwrap().has_class("lead"));
        assert_eq!(dom.text_content(p), "One two");
        assert_eq!(dom.element_index(p), 0);
    }

    #[test]
    fn handles_deep_nesting() {
        let mut html = String::new();
        for _ in 0..3000 {
            html.push_str("<div>");
        }
        html.push_str("deep");
        let dom = parse_html(html.as_bytes());
        assert!(!dom.nodes.is_empty());
    }
}
