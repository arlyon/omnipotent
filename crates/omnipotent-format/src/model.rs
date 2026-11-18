//! Presentation-independent articles. IDs and link states are descriptive;
//! this module performs no URL resolution, fetching, or filesystem access.

use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::{fmt, num::NonZeroU32};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierError {
    Empty,
    InvalidSource,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "identifier must not be empty",
            Self::InvalidSource => {
                "source must contain only ASCII letters, digits, '.', ':', '-' or '_'"
            }
        })
    }
}

impl core::error::Error for IdentifierError {}

/// A stable logical namespace, such as `wikipedia:en` or `publisher:manuals`.
/// It is not a mirror URL, filesystem path, or sync-connector instance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SourceId(String);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'-' | b'_'))
        {
            return Err(IdentifierError::InvalidSource);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SourceId {
    type Error = IdentifierError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SourceId> for String {
    fn from(value: SourceId) -> Self {
        value.0
    }
}

/// An opaque, nonempty identifier local to an article or collection.
/// It is never interpreted as a filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalId(String);

impl LocalId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LocalId {
    type Error = IdentifierError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<LocalId> for String {
    fn from(value: LocalId) -> Self {
        value.0
    }
}

/// An opaque source-assigned key, stable across revisions, titles, and mirrors.
/// Exact UTF-8 bytes are preserved; no path parsing or normalization is applied.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceKey(String);

impl ResourceKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ResourceKey {
    type Error = IdentifierError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ResourceKey> for String {
    fn from(value: ResourceKey) -> Self {
        value.0
    }
}

/// An opaque upstream revision (for example an ETag, digest, or version token).
/// It has equality but deliberately no chronological ordering.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RevisionId(String);

impl RevisionId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RevisionId {
    type Error = IdentifierError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RevisionId> for String {
    fn from(value: RevisionId) -> Self {
        value.0
    }
}

/// Revision-local anchor identity, never a durable cross-revision address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub NonZeroU32);

/// Search/retrieval identity within one collection generation. Zero is valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(pub u32);

/// Source-qualified identity shared by articles and cross-source references.
/// Collection-local document IDs, titles, and current locations are not identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceId {
    pub source: SourceId,
    pub key: ResourceKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: RevisionId,
    /// Original source timestamp; no chronology is inferred by this model.
    pub modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub identifier: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub attribution: Option<String>,
}

/// Uninterpreted source bytes, including unknown source fields or part types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueData {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Optional source URI, not an identity, fetch instruction, or local path.
    pub origin_uri: Option<String>,
    pub language: Option<String>,
    pub revision: Option<Revision>,
    /// Empty means licence information is unavailable, not unrestricted reuse.
    pub licenses: Vec<License>,
    pub extra: BTreeMap<LocalId, OpaqueData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Article {
    pub id: ResourceId,
    pub title: String,
    pub description: Option<String>,
    pub abstract_text: Option<Text>,
    pub provenance: Provenance,
    pub parts: Vec<Node>,
    pub tables: Vec<Table>,
    pub references: Vec<Reference>,
    pub assets: Vec<Asset>,
    pub links: Vec<Link>,
    pub extra: BTreeMap<LocalId, OpaqueData>,
}

/// Confirmed inline positions are UTF-8 byte ranges, never character counts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub value: String,
    pub spans: Vec<InlineSpan>,
    /// Block-associated links/citations without reliable inline positions.
    pub associated: Vec<Association>,
}

impl Text {
    pub fn plain(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineSpan {
    pub start: u32,
    pub end: u32,
    pub target: InlineTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InlineTarget {
    Link(LocalId),
    Citation(LocalId),
    Emphasis,
    Strong,
    Code,
    Subscript,
    Superscript,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Association {
    Link(LocalId),
    Citation {
        reference: LocalId,
        /// Retain the source marker even when its inline position is unknown.
        label: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub content: Part,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Part {
    Section {
        heading: Option<Text>,
        children: Vec<Node>,
    },
    Paragraph(Text),
    List {
        style: ListStyle,
        items: Vec<ListItem>,
    },
    Infobox {
        heading: Option<Text>,
        children: Vec<Node>,
    },
    Field {
        label: Text,
        children: Vec<Node>,
    },
    /// Table and citation relationships may legitimately be dangling upstream.
    Table(LocalId),
    Image(LocalId),
    Gallery(Vec<LocalId>),
    Unknown {
        source_type: String,
        original: OpaqueData,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ListStyle {
    Unordered,
    Ordered { start: u32 },
    Definition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListItem {
    pub label: Option<Text>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub id: LocalId,
    pub caption: Option<Text>,
    pub headers: Vec<Vec<Cell>>,
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub text: Text,
    pub row_span: NonZeroU32,
    pub column_span: NonZeroU32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: LocalId,
    pub text: Text,
    pub original: Option<OpaqueData>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: LocalId,
    pub origin_uri: Option<String>,
    pub media_type: String,
    pub caption: Option<Text>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub availability: AssetAvailability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AssetAvailability {
    NotInstalled,
    Installed { object: LocalId, length: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub id: LocalId,
    /// Preserve the source destination when available; typed links need no URL.
    pub original_target: Option<String>,
    pub label: Option<String>,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LinkTarget {
    Article(ArticleTarget),
    Fragment(FragmentTarget),
    Asset(LocalId),
    External,
    Unresolved { reason: String },
    Blocked { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArticleLocator {
    Id(ResourceId),
    Title { source: SourceId, title: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArticleMatch {
    Canonical,
    Alias { title: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ArticleTarget {
    Installed {
        id: ResourceId,
        matched_by: ArticleMatch,
        fragment: FragmentTarget,
    },
    NotInstalled {
        locator: ArticleLocator,
        source_fragment: Option<String>,
    },
    Unresolved {
        locator: ArticleLocator,
        source_fragment: Option<String>,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FragmentTarget {
    Absent,
    Resolved { source: String, node: NodeId },
    Unresolved { source: String },
}

/// Missing relationships remain explicit instead of being silently deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution<'a, T> {
    Found(&'a T),
    Missing(&'a LocalId),
}

impl Article {
    pub fn table<'a>(&'a self, id: &'a LocalId) -> Resolution<'a, Table> {
        self.tables
            .iter()
            .find(|v| &v.id == id)
            .map_or(Resolution::Missing(id), Resolution::Found)
    }

    pub fn reference<'a>(&'a self, id: &'a LocalId) -> Resolution<'a, Reference> {
        self.references
            .iter()
            .find(|v| &v.id == id)
            .map_or(Resolution::Missing(id), Resolution::Found)
    }

    pub fn asset<'a>(&'a self, id: &'a LocalId) -> Resolution<'a, Asset> {
        self.assets
            .iter()
            .find(|v| &v.id == id)
            .map_or(Resolution::Missing(id), Resolution::Found)
    }

    pub fn link<'a>(&'a self, id: &'a LocalId) -> Resolution<'a, Link> {
        self.links
            .iter()
            .find(|v| &v.id == id)
            .map_or(Resolution::Missing(id), Resolution::Found)
    }
}
