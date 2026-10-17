use super::{Result, invalid};
use quick_xml::{
    XmlVersion,
    events::{Event, attributes::Attribute},
    name::ResolveResult,
    reader::NsReader,
};
use std::collections::BTreeSet;

pub(super) const ITUNES: &str = "http://www.itunes.com/dtds/podcast-1.0.dtd";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Namespace {
    Rss,
    Itunes,
    Other,
}

pub(super) struct Node {
    pub namespace: Namespace,
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn named(&self, namespace: Namespace, name: &str) -> bool {
        self.namespace == namespace && self.name == name
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub(super) fn read(bytes: &[u8]) -> Result<Node> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let input = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if !input.chars().all(xml_character) {
        return Err(invalid());
    }
    let mut reader = NsReader::from_str(input);
    reader.config_mut().enable_all_checks(true);
    let mut stack = Vec::new();
    let mut root = None;
    let mut nodes = 0;
    let mut declared = false;
    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(|_| invalid())?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                nodes += 1;
                if stack.len() >= 64 || nodes > 100_000 || (stack.is_empty() && root.is_some()) {
                    return Err(invalid());
                }
                let namespace = match namespace {
                    ResolveResult::Unbound => Namespace::Rss,
                    ResolveResult::Bound(value) if value.into_inner() == ITUNES => {
                        Namespace::Itunes
                    }
                    ResolveResult::Bound(_) => Namespace::Other,
                    ResolveResult::Unknown(_) => return Err(invalid()),
                };
                let name = start.local_name().into_inner();
                if start.name().as_ref().len() > 256 || !qualified_name(start.name().as_ref()) {
                    return Err(invalid());
                }
                let mut attributes = Vec::new();
                let mut expanded_names = BTreeSet::new();
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(|_| invalid())?;
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|_| invalid())?;
                    if attributes.len() >= 32
                        || attribute.key.as_ref().len() > 256
                        || !qualified_name(attribute.key.as_ref())
                        || value.len() > 4096
                        || !value.chars().all(xml_character)
                    {
                        return Err(invalid());
                    }
                    if attribute.key.as_namespace_binding().is_none() {
                        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
                        let namespace = match namespace {
                            ResolveResult::Unbound => None,
                            ResolveResult::Bound(namespace) => {
                                let raw = namespace.into_inner();
                                if raw.len() > 4096 {
                                    return Err(invalid());
                                }
                                let raw_namespace = Attribute {
                                    key: attribute.key,
                                    value: raw.into(),
                                };
                                Some(
                                    raw_namespace
                                        .normalized_value(XmlVersion::Implicit1_0)
                                        .map_err(|_| invalid())?
                                        .into_owned(),
                                )
                            }
                            ResolveResult::Unknown(_) => return Err(invalid()),
                        };
                        if !expanded_names.insert((namespace, local.into_inner().to_owned())) {
                            return Err(invalid());
                        }
                    }
                    attributes.push((attribute.key.as_ref().to_owned(), value.into_owned()));
                }
                stack.push(Node {
                    namespace,
                    name: name.to_owned(),
                    attributes,
                    text: String::new(),
                    children: Vec::new(),
                });
                if empty {
                    close(&mut stack, &mut root)?;
                }
            }
            Event::End(_) => close(&mut stack, &mut root)?,
            Event::Text(text) => append(&mut stack, &text.xml10_content())?,
            Event::CData(text) => {
                if stack.is_empty() {
                    return Err(invalid());
                }
                append(&mut stack, &text.xml10_content())?;
            }
            Event::GeneralRef(reference) => {
                let resolved = reference.resolve_char_ref().map_err(|_| invalid())?;
                if let Some(character) = resolved {
                    if !xml_character(character) {
                        return Err(invalid());
                    }
                    append(&mut stack, &character.to_string())?;
                } else {
                    let value = match &*reference {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "apos" => "'",
                        "quot" => "\"",
                        _ => return Err(invalid()),
                    };
                    append(&mut stack, value)?;
                }
            }
            Event::Decl(declaration) => {
                if declared
                    || root.is_some()
                    || !stack.is_empty()
                    || declaration.version().map_err(|_| invalid())?.as_ref() != "1.0"
                {
                    return Err(invalid());
                }
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|_| invalid())?;
                    if !encoding.eq_ignore_ascii_case("utf-8")
                        && !encoding.eq_ignore_ascii_case("us-ascii")
                    {
                        return Err(invalid());
                    }
                }
                declared = true;
            }
            Event::DocType(_) => return Err(invalid()),
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(invalid());
    }
    root.ok_or_else(invalid)
}

fn close(stack: &mut Vec<Node>, root: &mut Option<Node>) -> Result<()> {
    let node = stack.pop().ok_or_else(invalid)?;
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(invalid());
    }
    Ok(())
}

fn append(stack: &mut [Node], text: &str) -> Result<()> {
    let Some(node) = stack.last_mut() else {
        return if text.trim().is_empty() {
            Ok(())
        } else {
            Err(invalid())
        };
    };
    if text.trim().is_empty()
        && node.namespace == Namespace::Rss
        && matches!(node.name.as_str(), "rss" | "channel" | "item")
    {
        return Ok(());
    }
    if node.text.len().saturating_add(text.len()) > 16_384 {
        return Err(invalid());
    }
    node.text.push_str(text);
    Ok(())
}

fn xml_character(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn qualified_name(value: &str) -> bool {
    let mut parts = value.split(':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), None, None) => unqualified_name(local),
        (Some(prefix), Some(local), None) => unqualified_name(prefix) && unqualified_name(local),
        _ => false,
    }
}

fn unqualified_name(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(name_start)
        && characters.all(|character| {
            name_start(character)
                || matches!(character,
            '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
        })
}

fn name_start(character: char) -> bool {
    matches!(character,
        'A'..='Z' | '_' | 'a'..='z'
        | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}'
        | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}'
        | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}'
        | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}
