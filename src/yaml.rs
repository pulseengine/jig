//! A small YAML tree with line numbers, built from yaml-rust2 parser events.
//!
//! Scalars stay strings (so `on:` is the key `on`). The tree fails closed: a
//! document that does not parse, has more than one document, a duplicate key,
//! a non-scalar key, a `<<` merge key (yaml-rust2 does not apply merges, so
//! merged values would be invisible) or expands past a size budget through
//! aliases is an error, never a partial read.

use std::collections::HashMap;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

/// Largest input read, in bytes.
pub const MAX_BYTES: usize = 1 << 20;
/// Most nodes a document may expand to once aliases are resolved.
const MAX_NODES: u64 = 1_000_000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub key: String,
    pub line: usize,
    pub value: Node,
}

#[derive(Clone, Debug)]
pub enum Node {
    Scalar { value: String, line: usize },
    Seq { items: Vec<Node>, line: usize },
    Map { entries: Vec<Entry>, line: usize },
}

impl Node {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Node::Scalar { line, .. } | Node::Seq { line, .. } | Node::Map { line, .. } => *line,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map { entries, .. } => entries.iter().find(|e| e.key == key).map(|e| &e.value),
            _ => None,
        }
    }
}

enum Frame {
    Seq {
        anchor: usize,
        line: usize,
        items: Vec<Node>,
        size: u64,
    },
    Map {
        anchor: usize,
        line: usize,
        entries: Vec<Entry>,
        key: Option<(String, usize)>,
        size: u64,
    },
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    root: Option<Node>,
    anchors: HashMap<usize, (Node, u64)>,
    docs: usize,
    err: Option<String>,
}

const TOO_BIG: &str = "expands to too many nodes through aliases";

impl Builder {
    fn fail(&mut self, msg: &str) {
        if self.err.is_none() {
            self.err = Some(msg.to_string());
        }
    }

    fn value(&mut self, node: Node, size: u64, anchor: usize) {
        if anchor != 0 {
            self.anchors.insert(anchor, (node.clone(), size));
        }
        let mut err = None;
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some(Frame::Seq { items, size: s, .. }) => {
                items.push(node);
                *s = s.saturating_add(size);
                if *s > MAX_NODES {
                    err = Some(TOO_BIG);
                }
            }
            Some(Frame::Map {
                entries,
                key,
                size: s,
                ..
            }) => {
                *s = s.saturating_add(size);
                if *s > MAX_NODES {
                    err = Some(TOO_BIG);
                } else {
                    match key.take() {
                        None => match node {
                            Node::Scalar { value, line } => *key = Some((value, line)),
                            _ => err = Some("a mapping key is not a scalar"),
                        },
                        Some((k, line)) => {
                            if entries.iter().any(|e| e.key == k) {
                                err = Some("duplicate mapping key");
                            }
                            entries.push(Entry {
                                key: k,
                                line,
                                value: node,
                            });
                        }
                    }
                }
            }
        }
        if let Some(e) = err {
            self.fail(e);
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        if self.err.is_some() {
            return;
        }
        let line = mark.line();
        match ev {
            Event::DocumentStart => {
                self.docs += 1;
                if self.docs > 1 {
                    self.fail("more than one YAML document");
                }
            }
            Event::Scalar(value, _, anchor, _) => {
                self.value(Node::Scalar { value, line }, 1, anchor);
            }
            Event::SequenceStart(anchor, _) => self.stack.push(Frame::Seq {
                anchor,
                line,
                items: Vec::new(),
                size: 1,
            }),
            Event::MappingStart(anchor, _) => self.stack.push(Frame::Map {
                anchor,
                line,
                entries: Vec::new(),
                key: None,
                size: 1,
            }),
            Event::SequenceEnd | Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Seq {
                    anchor,
                    line,
                    items,
                    size,
                }) => self.value(Node::Seq { items, line }, size, anchor),
                Some(Frame::Map {
                    anchor,
                    line,
                    entries,
                    key,
                    size,
                }) => {
                    if key.is_some() {
                        self.fail("a mapping key has no value");
                    }
                    self.value(Node::Map { entries, line }, size, anchor)
                }
                None => self.fail("unbalanced YAML events"),
            },
            Event::Alias(id) => match self.anchors.get(&id) {
                None => self.fail("alias to an unknown anchor"),
                Some((n, size)) => {
                    let (n, size) = (n.clone(), *size);
                    self.value(n, size, 0);
                }
            },
            _ => {}
        }
    }
}

fn has_merge_key(n: &Node) -> bool {
    match n {
        Node::Scalar { .. } => false,
        Node::Seq { items, .. } => items.iter().any(has_merge_key),
        Node::Map { entries, .. } => entries
            .iter()
            .any(|e| e.key == "<<" || has_merge_key(&e.value)),
    }
}

/// Parse one YAML document. The error text says why it was not read.
pub fn parse(text: &str) -> Result<Node, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("is larger than {MAX_BYTES} bytes"));
    }
    let mut b = Builder::default();
    Parser::new_from_str(text)
        .load(&mut b, true)
        .map_err(|e| format!("cannot be read as YAML ({e})"))?;
    if let Some(e) = b.err {
        return Err(e);
    }
    let root = b.root.ok_or_else(|| "is empty".to_string())?;
    if has_merge_key(&root) {
        return Err(
            "uses a YAML merge key (<<); merged values are not checked, so not passed".into(),
        );
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_one_based_and_block_scalars_start_at_content() {
        let n = parse("a: 1\nb:\n  run: |\n    x\n").unwrap();
        let run = n.get("b").unwrap().get("run").unwrap();
        assert_eq!(n.get("a").unwrap().line(), 1);
        assert_eq!(run.line(), 4);
    }

    #[test]
    fn anchors_resolve_and_bombs_are_refused() {
        let n = parse("a: &x [1, 2]\nb: *x\n").unwrap();
        assert!(matches!(n.get("b"), Some(Node::Seq { .. })));
        let mut s = String::from("l0: &l0 [a,a,a,a,a,a,a,a,a,a]\n");
        for i in 1..9 {
            let p = i - 1;
            let refs = format!("*l{p},").repeat(10);
            s += &format!("l{i}: &l{i} [{refs}]\n");
        }
        assert!(parse(&s).is_err());
    }
}
