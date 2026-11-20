//! Shared resource metadata and passage locations for packed article bundles.
use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::{DocumentId, LocalId, NodeId, Provenance, ResourceId};
use core::num::NonZeroU32;
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned, byteorder::little_endian::U32,
};

pub const ARTICLE_MEDIA_TYPE: &str = "application/vnd.omnipotent.article+binary";
pub const MAX_ARTICLE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Article,
    Publication,
    Map,
    Media,
    Data,
}

/// Object IDs are opaque package-local keys, never filesystem paths or URLs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub object: LocalId,
    pub length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub id: ResourceId,
    pub kind: ResourceKind,
    pub title: String,
    pub media_type: String,
    pub provenance: Provenance,
    pub payload: ObjectRef,
}

impl Resource {
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
            && self.media_type.contains('/')
            && self.media_type.bytes().all(|b| b.is_ascii_graphic())
            && (self.kind != ResourceKind::Article || self.media_type == ARTICLE_MEDIA_TYPE)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SearchLocation {
    /// None denotes the abstract; otherwise offsets address a paragraph's Text.
    Article {
        node: Option<NodeId>,
        start: u32,
        end: u32,
    },
    /// A native-format locator, e.g. an EPUB fragment or PDF page fragment.
    /// Interpretation belongs to that format's adapter, not the package reader.
    Native { fragment: Option<String> },
}

/// Exactly 16 stored bytes, in the same row order as the bundle's codes and vectors.
/// Node zero denotes the abstract. Text and headings live only in the article.
#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct PassageRef {
    resource: U32,
    node: U32,
    start: U32,
    end: U32,
}
impl PassageRef {
    pub fn new(
        resource: DocumentId,
        node: Option<NodeId>,
        start: u32,
        end: u32,
    ) -> Result<Self, crate::RecordError> {
        if start >= end {
            return Err(crate::RecordError::Invalid);
        }
        Ok(Self {
            resource: U32::new(resource.0),
            node: U32::new(node.map_or(0, |n| n.0.get())),
            start: U32::new(start),
            end: U32::new(end),
        })
    }
    pub fn parse(bytes: &[u8]) -> Result<&Self, crate::RecordError> {
        let r = Self::ref_from_bytes(bytes).map_err(|_| crate::RecordError::Size)?;
        if r.start.get() >= r.end.get() {
            return Err(crate::RecordError::Invalid);
        }
        Ok(r)
    }
    pub fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }
    pub fn resource(&self) -> DocumentId {
        DocumentId(self.resource.get())
    }
    pub fn remap(&mut self, resource: DocumentId) {
        self.resource = U32::new(resource.0);
    }
    pub fn node(&self) -> Option<NodeId> {
        NonZeroU32::new(self.node.get()).map(NodeId)
    }
    pub fn start(&self) -> u32 {
        self.start.get()
    }
    pub fn end(&self) -> u32 {
        self.end.get()
    }
}
const _: [(); 16] = [(); core::mem::size_of::<PassageRef>()];

/// In-memory preparation/search result; its text/heading are never persisted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchEntry {
    pub resource: DocumentId,
    pub location: SearchLocation,
    pub heading: String,
    pub text: String,
}

impl SearchEntry {
    pub fn matches(&self, resource: &Resource) -> bool {
        if self.text.trim().is_empty() {
            return false;
        }
        match &self.location {
            SearchLocation::Article { start, end, .. } => {
                resource.kind == ResourceKind::Article
                    && start < end
                    && (end - start) as usize == self.text.len()
            }
            SearchLocation::Native { fragment } => {
                resource.kind != ResourceKind::Article
                    && fragment.as_ref().is_none_or(|f| !f.is_empty())
            }
        }
    }
}

/// Model identity and actual asset hashes, shared by host/client query encoders.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingProfile {
    pub model: String,
    pub files: alloc::collections::BTreeMap<String, String>,
    pub dimensions: usize,
    pub document_template: String,
    pub query_template: String,
}
