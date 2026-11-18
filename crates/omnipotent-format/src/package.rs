//! Shared mixed-resource package schema. Payloads live in blob storage; only
//! Articles use our structured JSON representation. Native files stay unchanged.
use alloc::string::String;
use serde::{Deserialize, Serialize};

use crate::{DocumentId, LocalId, NodeId, Provenance, ResourceId};

pub const PACKAGE_VERSION: u32 = 2;
pub const ARTICLE_MEDIA_TYPE: &str = "application/vnd.omnipotent.article+json";
pub const MAX_METADATA_BYTES: usize = 256 * 1024;
pub const MAX_ARTICLE_BYTES: usize = 16 * 1024 * 1024;
pub const EMBEDDING_DIMENSIONS: usize = 768;
pub const VECTOR_INDEX_NAME: &str = "passage_embeddings";

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

/// The query-facing subset of the assembler's training settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorIndex {
    pub dimensions: u32,
    pub clusters: u32,
    pub subvectors: u32,
    pub retain_raw_for_rerank: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchHit {
    pub vector: u64,
    pub distance: f32,
    pub entry: SearchEntry,
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

pub fn supported_dimensions(dimensions: usize) -> bool {
    [128, 256, 512, 768].contains(&dimensions)
}
