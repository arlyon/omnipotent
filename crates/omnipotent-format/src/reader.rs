//! Transaction-bound package reads on the same schema used by the host packer.
//! No filesystem, decompressor, network stack, or inference runtime is required.
use alloc::{format, string::String, vec, vec::Vec};
use core::fmt;

use shodh_redb::{
    BlobId, BlobReader, FixedVec, IvfPqIndexDefinition, ReadTransaction, SearchParams,
    TableDefinition,
};

use crate::package::*;
use crate::{Article, AssetAvailability, DocumentId, LocalId, ModelLimits, Resolution, ResourceId};

pub const META: TableDefinition<&str, &[u8]> = TableDefinition::new("package");
pub const RESOURCES: TableDefinition<u32, &[u8]> = TableDefinition::new("resources");
pub const IDENTITIES: TableDefinition<&[u8], u32> = TableDefinition::new("identities");
pub const OBJECTS: TableDefinition<&str, BlobId> = TableDefinition::new("objects");
pub const PASSAGES: TableDefinition<u64, &[u8]> = TableDefinition::new("passages");
pub const VECTORS: TableDefinition<u64, FixedVec<EMBEDDING_DIMENSIONS>> =
    TableDefinition::new("raw_vectors");

#[derive(Debug)]
pub enum PackageError {
    Storage(String),
    Json(serde_json::Error),
    Invalid(&'static str),
    Missing(&'static str),
}
impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(message) => write!(f, "package storage: {message}"),
            Self::Json(error) => write!(f, "package JSON: {error}"),
            Self::Invalid(message) => write!(f, "invalid package: {message}"),
            Self::Missing(message) => write!(f, "package item unavailable: {message}"),
        }
    }
}
impl core::error::Error for PackageError {}
impl From<serde_json::Error> for PackageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
fn storage(error: impl fmt::Display) -> PackageError {
    PackageError::Storage(format!("{error}"))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, PackageError> {
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(PackageError::Invalid("metadata exceeds byte limit"));
    }
    Ok(serde_json::from_slice(bytes)?)
}

impl VectorIndex {
    pub fn definition(&self) -> Result<IvfPqIndexDefinition, PackageError> {
        if !supported_dimensions(self.dimensions as usize)
            || self.clusters == 0
            || self.subvectors == 0
            || self.dimensions % self.subvectors != 0
        {
            return Err(PackageError::Invalid("vector index settings"));
        }
        let def = IvfPqIndexDefinition::new(
            VECTOR_INDEX_NAME,
            self.dimensions,
            self.clusters,
            self.subvectors,
            shodh_redb::DistanceMetric::Cosine,
        );
        Ok(if self.retain_raw_for_rerank {
            def.with_raw_vectors()
        } else {
            def
        })
    }
}

pub struct PackageReader<'txn> {
    transaction: &'txn ReadTransaction,
}

impl<'txn> PackageReader<'txn> {
    pub fn open(transaction: &'txn ReadTransaction) -> Result<Self, PackageError> {
        let meta = transaction.open_table(META).map_err(storage)?;
        let version = meta
            .get("schema")
            .map_err(storage)?
            .ok_or(PackageError::Missing("package schema"))?;
        if version.value() != PACKAGE_VERSION.to_le_bytes() {
            return Err(PackageError::Invalid(
                "unsupported package schema; repack required",
            ));
        }
        Ok(Self { transaction })
    }

    pub fn resource(&self, document: DocumentId) -> Result<Resource, PackageError> {
        let table = self.transaction.open_table(RESOURCES).map_err(storage)?;
        let value = table
            .get(document.0)
            .map_err(storage)?
            .ok_or(PackageError::Missing("resource"))?;
        let resource: Resource = decode(value.value())?;
        if !resource.is_valid() {
            return Err(PackageError::Invalid("resource metadata"));
        }
        Ok(resource)
    }

    pub fn find(&self, id: &ResourceId) -> Result<Option<DocumentId>, PackageError> {
        let key = serde_json::to_vec(id)?;
        let table = self.transaction.open_table(IDENTITIES).map_err(storage)?;
        let found = table
            .get(key.as_slice())
            .map_err(storage)?
            .map(|v| DocumentId(v.value()));
        Ok(found)
    }

    /// Opens metadata only. Read into caller buffers or seek within native files.
    /// Range reads do not independently verify the whole-blob checksum.
    pub fn object(&self, object: &ObjectRef) -> Result<BlobReader<'txn>, PackageError> {
        let table = self.transaction.open_table(OBJECTS).map_err(storage)?;
        let id = table
            .get(object.object.as_str())
            .map_err(storage)?
            .ok_or(PackageError::Missing("object"))?
            .value();
        let reader = self
            .transaction
            .blob_reader(&id)
            .map_err(storage)?
            .ok_or(PackageError::Missing("blob"))?;
        if reader.len() != object.length {
            return Err(PackageError::Invalid("object length mismatch"));
        }
        Ok(reader)
    }

    pub fn payload(&self, document: DocumentId) -> Result<BlobReader<'txn>, PackageError> {
        self.object(&self.resource(document)?.payload)
    }

    /// Article JSON is decoded here, not imposed as a client-side codec.
    /// Native payloads must use `payload` instead; they are never parsed as Articles.
    pub fn article(
        &self,
        document: DocumentId,
        limits: ModelLimits,
    ) -> Result<Article, PackageError> {
        let resource = self.resource(document)?;
        if resource.kind != ResourceKind::Article {
            return Err(PackageError::Invalid("resource is not an article"));
        }
        let mut reader = self.object(&resource.payload)?;
        if reader.len() > MAX_ARTICLE_BYTES as u64 {
            return Err(PackageError::Invalid("article exceeds byte limit"));
        }
        let mut bytes = vec![0; reader.len() as usize];
        let mut offset = 0;
        while offset < bytes.len() {
            let count = reader.read_into(&mut bytes[offset..]).map_err(storage)?;
            if count == 0 {
                return Err(PackageError::Invalid("truncated article blob"));
            }
            offset += count;
        }
        let article: Article = serde_json::from_slice(&bytes)?;
        article
            .validate(limits)
            .map_err(|e| PackageError::Storage(format!("article model: {e}")))?;
        if article.id != resource.id {
            return Err(PackageError::Invalid("article/catalogue identity mismatch"));
        }
        Ok(article)
    }

    pub fn asset(
        &self,
        article: &Article,
        asset: &LocalId,
    ) -> Result<BlobReader<'txn>, PackageError> {
        let Resolution::Found(asset) = article.asset(asset) else {
            return Err(PackageError::Missing("article asset"));
        };
        match &asset.availability {
            AssetAvailability::Installed { object, length } => self.object(&ObjectRef {
                object: object.clone(),
                length: *length,
            }),
            AssetAvailability::NotInstalled => Err(PackageError::Missing("asset is not installed")),
        }
    }

    pub fn search_entry(&self, id: u64) -> Result<SearchEntry, PackageError> {
        let table = self.transaction.open_table(PASSAGES).map_err(storage)?;
        let value = table
            .get(id)
            .map_err(storage)?
            .ok_or(PackageError::Missing("search entry"))?;
        let entry: SearchEntry = decode(value.value())?;
        let resource = self.resource(entry.resource)?;
        if !entry.matches(&resource) {
            return Err(PackageError::Invalid(
                "search target does not match resource kind",
            ));
        }
        Ok(entry)
    }

    pub fn embedding_profile(&self) -> Result<EmbeddingProfile, PackageError> {
        let meta = self.transaction.open_table(META).map_err(storage)?;
        let value = meta
            .get("embedding_profile")
            .map_err(storage)?
            .ok_or(PackageError::Missing("embedding profile"))?;
        decode(value.value())
    }

    pub fn search(
        &self,
        profile: &EmbeddingProfile,
        query: &[f32],
        params: &SearchParams,
    ) -> Result<Vec<SearchHit>, PackageError> {
        if *profile != self.embedding_profile()? {
            return Err(PackageError::Invalid("query embedding profile mismatch"));
        }
        let meta = self.transaction.open_table(META).map_err(storage)?;
        let value = meta
            .get("vector_index")
            .map_err(storage)?
            .ok_or(PackageError::Missing("vector index"))?;
        let settings: VectorIndex = decode(value.value())?;
        if query.len() != settings.dimensions as usize
            || query.iter().any(|v| !v.is_finite())
            || params.k == 0
            || params.k > 1000
            || params.candidates > 10_000
            || params.nprobe == 0
            || params.nprobe > settings.clusters
        {
            return Err(PackageError::Invalid("query dimensions or search budget"));
        }
        let index = self
            .transaction
            .open_ivfpq_index(&settings.definition()?)
            .map_err(storage)?;
        let neighbors = index
            .search(self.transaction, query, params)
            .map_err(storage)?;
        neighbors
            .into_iter()
            .map(|hit| {
                Ok(SearchHit {
                    vector: hit.key,
                    distance: hit.distance,
                    entry: self.search_entry(hit.key)?,
                })
            })
            .collect()
    }
}
