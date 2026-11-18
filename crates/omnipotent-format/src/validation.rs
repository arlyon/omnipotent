use alloc::{collections::BTreeSet, vec::Vec};
use core::fmt;

use crate::model::*;

/// Validation budgets, not bounds on allocations already made by a deserializer.
/// A codec must independently bound its input size and recursion before decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_items: usize,
    pub max_text_bytes: usize,
    pub max_spans: usize,
}

impl Default for ModelLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_nodes: 10_000,
            max_items: 100_000,
            max_text_bytes: 16 * 1024 * 1024,
            max_spans: 100_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    EmptyField(&'static str),
    DuplicateId { kind: &'static str, id: LocalId },
    DuplicateNode(NodeId),
    MissingLink(LocalId),
    MissingAnchor(NodeId),
    InvalidSpan { start: u32, end: u32 },
    DepthLimit,
    NodeLimit,
    ItemLimit,
    TextLimit,
    SpanLimit,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for ModelError {}

struct Validator<'a> {
    limits: ModelLimits,
    links: BTreeSet<&'a LocalId>,
    text_bytes: usize,
    spans: usize,
    items: usize,
}

impl Article {
    /// Validate IDs, confirmed UTF-8 spans, local anchors, and structural budgets.
    /// Missing source tables, references, or assets remain representable through
    /// `Resolution::Missing`. Cross-article resolution is a collection concern.
    pub fn validate(&self, limits: ModelLimits) -> Result<(), ModelError> {
        nonempty(&self.title, "title")?;
        if let Some(uri) = &self.provenance.origin_uri {
            nonempty(uri, "origin_uri")?;
        }
        if let Some(language) = &self.provenance.language {
            nonempty(language, "language")?;
        }
        let mut validator = Validator {
            limits,
            links: BTreeSet::new(),
            text_bytes: 0,
            spans: 0,
            items: 0,
        };
        validator.items(self.links.len())?;
        validator.items(self.tables.len())?;
        validator.items(self.references.len())?;
        validator.items(self.assets.len())?;
        validator.items(self.extra.len())?;
        validator.items(self.provenance.extra.len())?;
        validator.items(self.provenance.licenses.len())?;
        validator.links = unique_ids("link", self.links.iter().map(|v| &v.id))?;
        unique_ids("table", self.tables.iter().map(|v| &v.id))?;
        unique_ids("reference", self.references.iter().map(|v| &v.id))?;
        unique_ids("asset", self.assets.iter().map(|v| &v.id))?;
        validator.string(&self.title)?;
        if let Some(value) = &self.description {
            validator.string(value)?;
        }
        if let Some(value) = &self.abstract_text {
            validator.text(value)?;
        }
        for license in &self.provenance.licenses {
            nonempty(&license.identifier, "license.identifier")?;
        }
        for table in &self.tables {
            if let Some(caption) = &table.caption {
                validator.text(caption)?;
            }
            validator.items(table.headers.len())?;
            validator.items(table.rows.len())?;
            for row in table.headers.iter().chain(&table.rows) {
                validator.items(row.len())?;
                for cell in row {
                    validator.text(&cell.text)?;
                }
            }
        }
        for reference in &self.references {
            validator.text(&reference.text)?;
        }
        for asset in &self.assets {
            if let Some(uri) = &asset.origin_uri {
                nonempty(uri, "asset.origin_uri")?;
            }
            nonempty(&asset.media_type, "asset.media_type")?;
            if let Some(caption) = &asset.caption {
                validator.text(caption)?;
            }
        }

        let mut nodes = BTreeSet::new();
        let mut pending = Vec::new();
        enqueue(&mut pending, &self.parts, 1, nodes.len(), limits)?;
        while let Some((node, depth)) = pending.pop() {
            if depth > limits.max_depth {
                return Err(ModelError::DepthLimit);
            }
            if !nodes.insert(node.id) {
                return Err(ModelError::DuplicateNode(node.id));
            }
            if nodes.len() > limits.max_nodes {
                return Err(ModelError::NodeLimit);
            }
            match &node.content {
                Part::Paragraph(text) => validator.text(text)?,
                Part::Section { heading, children } | Part::Infobox { heading, children } => {
                    if let Some(heading) = heading {
                        validator.text(heading)?;
                    }
                    enqueue(&mut pending, children, depth + 1, nodes.len(), limits)?;
                }
                Part::Field { label, children } => {
                    validator.text(label)?;
                    enqueue(&mut pending, children, depth + 1, nodes.len(), limits)?;
                }
                Part::List { items, .. } => {
                    validator.items(items.len())?;
                    for item in items {
                        if let Some(label) = &item.label {
                            validator.text(label)?;
                        }
                        enqueue(&mut pending, &item.children, depth + 1, nodes.len(), limits)?;
                    }
                }
                Part::Unknown {
                    source_type,
                    original,
                } => {
                    nonempty(source_type, "unknown.source_type")?;
                    nonempty(&original.media_type, "unknown.media_type")?;
                }
                Part::Gallery(assets) => validator.items(assets.len())?,
                Part::Table(_) | Part::Image(_) => {}
            }
        }
        for link in &self.links {
            if let Some(target) = &link.original_target {
                nonempty(target, "link.original_target")?;
            }
            if matches!(link.target, LinkTarget::External) && link.original_target.is_none() {
                return Err(ModelError::EmptyField("link.original_target"));
            }
            let local_fragment = match &link.target {
                LinkTarget::Fragment(fragment) => Some(fragment),
                LinkTarget::Article(target) => match target {
                    ArticleTarget::Installed {
                        id,
                        matched_by,
                        fragment,
                    } => {
                        if let ArticleMatch::Alias { title } = matched_by {
                            nonempty(title, "alias.title")?;
                        }
                        validate_fragment(fragment)?;
                        if id == &self.id { Some(fragment) } else { None }
                    }
                    ArticleTarget::NotInstalled {
                        locator,
                        source_fragment,
                    } => {
                        validate_locator(locator)?;
                        validate_source_fragment(source_fragment)?;
                        None
                    }
                    ArticleTarget::Unresolved {
                        locator,
                        source_fragment,
                        reason,
                    } => {
                        validate_locator(locator)?;
                        validate_source_fragment(source_fragment)?;
                        nonempty(reason, "article.unresolved.reason")?;
                        None
                    }
                },
                LinkTarget::Unresolved { reason } | LinkTarget::Blocked { reason } => {
                    nonempty(reason, "link.reason")?;
                    None
                }
                LinkTarget::Asset(_) | LinkTarget::External => None,
            };
            if let Some(fragment) = local_fragment {
                validate_fragment(fragment)?;
                if let FragmentTarget::Resolved { node, .. } = fragment {
                    if !nodes.contains(node) {
                        return Err(ModelError::MissingAnchor(*node));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Validator<'_> {
    fn items(&mut self, count: usize) -> Result<(), ModelError> {
        self.items = self.items.checked_add(count).ok_or(ModelError::ItemLimit)?;
        if self.items > self.limits.max_items {
            return Err(ModelError::ItemLimit);
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), ModelError> {
        self.text_bytes = self
            .text_bytes
            .checked_add(value.len())
            .ok_or(ModelError::TextLimit)?;
        if self.text_bytes > self.limits.max_text_bytes {
            return Err(ModelError::TextLimit);
        }
        Ok(())
    }

    fn text(&mut self, text: &Text) -> Result<(), ModelError> {
        self.string(&text.value)?;
        self.spans = self
            .spans
            .checked_add(text.spans.len())
            .ok_or(ModelError::SpanLimit)?;
        if self.spans > self.limits.max_spans {
            return Err(ModelError::SpanLimit);
        }
        self.items(text.associated.len())?;
        for span in &text.spans {
            let start = usize::try_from(span.start).map_err(|_| ModelError::InvalidSpan {
                start: span.start,
                end: span.end,
            })?;
            let end = usize::try_from(span.end).map_err(|_| ModelError::InvalidSpan {
                start: span.start,
                end: span.end,
            })?;
            if start >= end || text.value.get(start..end).is_none() {
                return Err(ModelError::InvalidSpan {
                    start: span.start,
                    end: span.end,
                });
            }
            if let InlineTarget::Link(id) = &span.target {
                self.link(id)?;
            }
        }
        for associated in &text.associated {
            if let Association::Link(id) = associated {
                self.link(id)?;
            }
        }
        Ok(())
    }

    fn link(&self, id: &LocalId) -> Result<(), ModelError> {
        if !self.links.contains(id) {
            return Err(ModelError::MissingLink(id.clone()));
        }
        Ok(())
    }
}

fn enqueue<'a>(
    pending: &mut Vec<(&'a Node, usize)>,
    children: &'a [Node],
    depth: usize,
    visited: usize,
    limits: ModelLimits,
) -> Result<(), ModelError> {
    let count = visited
        .checked_add(pending.len())
        .and_then(|v| v.checked_add(children.len()))
        .ok_or(ModelError::NodeLimit)?;
    if count > limits.max_nodes {
        return Err(ModelError::NodeLimit);
    }
    if !children.is_empty() && depth > limits.max_depth {
        return Err(ModelError::DepthLimit);
    }
    pending.extend(children.iter().rev().map(|node| (node, depth)));
    Ok(())
}

fn unique_ids<'a>(
    kind: &'static str,
    ids: impl Iterator<Item = &'a LocalId>,
) -> Result<BTreeSet<&'a LocalId>, ModelError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(ModelError::DuplicateId {
                kind,
                id: id.clone(),
            });
        }
    }
    Ok(seen)
}

fn nonempty(value: &str, field: &'static str) -> Result<(), ModelError> {
    if value.trim().is_empty() {
        return Err(ModelError::EmptyField(field));
    }
    Ok(())
}

fn validate_locator(locator: &ArticleLocator) -> Result<(), ModelError> {
    if let ArticleLocator::Title { title, .. } = locator {
        nonempty(title, "locator.title")?;
    }
    Ok(())
}

fn validate_source_fragment(fragment: &Option<alloc::string::String>) -> Result<(), ModelError> {
    if let Some(fragment) = fragment {
        nonempty(fragment, "source_fragment")?;
    }
    Ok(())
}

fn validate_fragment(fragment: &FragmentTarget) -> Result<(), ModelError> {
    match fragment {
        FragmentTarget::Resolved { source, .. } | FragmentTarget::Unresolved { source } => {
            nonempty(source, "source_fragment")
        }
        FragmentTarget::Absent => Ok(()),
    }
}
