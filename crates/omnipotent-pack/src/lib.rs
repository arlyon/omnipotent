//! Shared package assembly operations used by the thin CLI.
#[cfg(test)]
mod tests;
use anyhow::{Context, Result, ensure};
use omnipotent_embed::{DIMENSIONS, Encoder, MAX_BATCH_SIZE, PreparedPassage, batch_len};
use omnipotent_format::{Article, DocumentId, LocalId, package::*, reader::*};
use serde::{Deserialize, Serialize};
use shodh_redb::{
    ContentType, Database, ReadOnlyDatabase, ReadableDatabase, ReadableTable,
    ReadableTableMetadata, StoreOptions, TableDefinition,
};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

const TOKENS: TableDefinition<u64, &[u8]> = TableDefinition::new("prepared_tokens");
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Preparing,
    Embedding,
    Complete,
    Ready,
}
#[derive(Serialize, Deserialize)]
struct Progress {
    stage: Stage,
    ready: bool,
}

pub struct Package {
    pub db: Database,
}
impl Package {
    pub fn create(path: &Path, profile: &EmbeddingProfile) -> Result<Self> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "creating new output {} (will not overwrite)",
                    path.display()
                )
            })?;
        let package = Self {
            db: Database::create(path)?,
        };
        let tx = package.db.begin_write()?;
        {
            let mut meta = tx.open_table(META)?;
            meta.insert("schema", PACKAGE_VERSION.to_le_bytes().as_slice())?;
            meta.insert("embedding_profile", serde_json::to_vec(profile)?.as_slice())?;
            meta.insert(
                "progress",
                serde_json::to_vec(&Progress {
                    stage: Stage::Preparing,
                    ready: false,
                })?
                .as_slice(),
            )?;
        }
        tx.open_table(RESOURCES)?;
        tx.open_table(IDENTITIES)?;
        tx.open_table(OBJECTS)?;
        tx.open_table(PASSAGES)?;
        tx.open_table(VECTORS)?;
        tx.open_table(TOKENS)?;
        tx.commit()?;
        Ok(package)
    }
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(path.is_file(), "package does not exist: {}", path.display());
        let package = Self {
            db: Database::open(path)?,
        };
        PackageReader::open(&package.db.begin_read()?)?;
        Ok(package)
    }
    pub fn stage(&self) -> Result<Stage> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(META)?;
        let bytes = table.get("progress")?.context("missing progress")?;
        Ok(serde_json::from_slice::<Progress>(bytes.value())?.stage)
    }
    fn set_stage(&self, stage: Stage) -> Result<()> {
        let tx = self.db.begin_write()?;
        tx.open_table(META)?.insert(
            "progress",
            serde_json::to_vec(&Progress {
                stage,
                ready: stage == Stage::Ready,
            })?
            .as_slice(),
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn profile(&self) -> Result<EmbeddingProfile> {
        Ok(PackageReader::open(&self.db.begin_read()?)?.embedding_profile()?)
    }
    pub fn next_document(&self) -> Result<DocumentId> {
        Ok(DocumentId(
            self.db
                .begin_read()?
                .open_table(RESOURCES)?
                .len()?
                .try_into()?,
        ))
    }
    pub fn add_article(&self, article: &Article, prepared: &[PreparedPassage]) -> Result<()> {
        ensure!(self.stage()? == Stage::Preparing, "preparation is closed");
        let bytes = serde_json::to_vec(article)?;
        ensure!(
            bytes.len() <= MAX_ARTICLE_BYTES,
            "article exceeds package size limit"
        );
        let resource = Resource {
            id: article.id.clone(),
            kind: ResourceKind::Article,
            title: article.title.clone(),
            media_type: ARTICLE_MEDIA_TYPE.into(),
            provenance: article.provenance.clone(),
            payload: ObjectRef {
                object: LocalId::new("pending")?,
                length: bytes.len() as u64,
            },
        };
        self.add_resource(resource, bytes.as_slice(), prepared)
    }
    pub fn add_native(&self, resource: Resource, path: &Path) -> Result<()> {
        ensure!(
            resource.kind != ResourceKind::Article,
            "use add_article for articles"
        );
        self.add_resource(resource, File::open(path)?, &[])
    }
    fn add_resource(
        &self,
        mut resource: Resource,
        mut payload: impl Read,
        prepared: &[PreparedPassage],
    ) -> Result<()> {
        ensure!(self.stage()? == Stage::Preparing, "preparation is closed");
        ensure!(resource.is_valid(), "invalid resource metadata");
        let tx = self.db.begin_write()?;
        let document: u32 = tx.open_table(RESOURCES)?.len()?.try_into()?;
        let identity = serde_json::to_vec(&resource.id)?;
        ensure!(
            tx.open_table(IDENTITIES)?
                .get(identity.as_slice())?
                .is_none(),
            "duplicate resource {:?}",
            resource.id
        );
        let object = format!("resource/{document}/payload");
        let mut writer = tx.blob_writer(
            ContentType::OctetStream,
            "resource",
            StoreOptions::default(),
        )?;
        let mut buffer = [0; 65536];
        let mut length = 0;
        loop {
            let count = payload.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            writer.write(&buffer[..count])?;
            length += count as u64;
        }
        let blob = writer.finish()?;
        resource.payload = ObjectRef {
            object: LocalId::new(object.clone())?,
            length,
        };
        let metadata = serde_json::to_vec(&resource)?;
        ensure!(
            metadata.len() <= MAX_METADATA_BYTES,
            "resource metadata exceeds package size limit"
        );
        tx.open_table(OBJECTS)?.insert(object.as_str(), blob)?;
        tx.open_table(RESOURCES)?
            .insert(document, metadata.as_slice())?;
        tx.open_table(IDENTITIES)?
            .insert(identity.as_slice(), document)?;
        let mut passages = tx.open_table(PASSAGES)?;
        let mut tokens = tx.open_table(TOKENS)?;
        for (id, passage) in (passages.len()?..).zip(prepared) {
            ensure!(
                passage.entry.resource == DocumentId(document) && passage.entry.matches(&resource),
                "passage location/resource mismatch"
            );
            batch_len([passage.tokens.len()], MAX_BATCH_SIZE)?;
            let bytes = serde_json::to_vec(&passage.entry)?;
            ensure!(
                bytes.len() <= MAX_METADATA_BYTES,
                "passage exceeds metadata size limit"
            );
            passages.insert(id, bytes.as_slice())?;
            let bytes: Vec<_> = passage
                .tokens
                .iter()
                .flat_map(|t| t.to_le_bytes())
                .collect();
            tokens.insert(id, bytes.as_slice())?;
        }
        drop(tokens);
        drop(passages);
        tx.commit()?;
        Ok(())
    }
    pub fn finish_preparation(&self) -> Result<()> {
        ensure!(self.stage()? == Stage::Preparing, "not preparing");
        self.set_stage(Stage::Embedding)
    }
    pub fn embed(&self, encoder: &Encoder, maximum: usize) -> Result<()> {
        ensure!(
            self.stage()? == Stage::Embedding,
            "finish preparation before embedding"
        );
        let mut descriptors = Vec::new();
        {
            let tx = self.db.begin_read()?;
            for row in tx.open_table(TOKENS)?.iter()? {
                let (id, bytes) = row?;
                ensure!(bytes.value().len() % 4 == 0, "invalid prepared token row");
                descriptors.push((u32::try_from(bytes.value().len() / 4)?, id.value()));
            }
        }
        eprintln!(
            "sorting {} descriptors ({} bytes)",
            descriptors.len(),
            descriptors.len() * std::mem::size_of::<(u32, u64)>()
        );
        descriptors.sort_unstable();
        let mut cursor = 0;
        while cursor < descriptors.len() {
            let count = batch_len(descriptors[cursor..].iter().map(|d| d.0 as usize), maximum)?;
            let batch = &descriptors[cursor..cursor + count];
            let rows = {
                let tx = self.db.begin_read()?;
                let tokens = tx.open_table(TOKENS)?;
                batch
                    .iter()
                    .map(|(_, id)| {
                        let bytes = tokens.get(*id)?.context("missing prepared tokens")?;
                        Ok(bytes
                            .value()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|v| u32::from_le_bytes(*v))
                            .collect())
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            let vectors = encoder.embed(&rows)?;
            let tx = self.db.begin_write()?;
            {
                let mut table = tx.open_table(VECTORS)?;
                let mut tokens = tx.open_table(TOKENS)?;
                for ((_, id), vector) in batch.iter().zip(vectors) {
                    table.insert(*id, vector)?;
                    tokens.remove(*id)?;
                }
            }
            tx.commit()?;
            cursor += count;
            eprintln!("embedded {cursor}/{}", descriptors.len());
        }
        self.finish_embedding()
    }
    pub fn finish_embedding(&self) -> Result<()> {
        ensure!(self.stage()? == Stage::Embedding, "not embedding");
        let tx = self.db.begin_read()?;
        ensure!(
            tx.open_table(TOKENS)?.is_empty()?
                && tx.open_table(PASSAGES)?.len()? == tx.open_table(VECTORS)?.len()?,
            "embedding is incomplete"
        );
        drop(tx);
        self.set_stage(Stage::Complete)
    }
    /// No source, tokenizer or model is needed here. Raw 768d vectors remain
    /// available; the search index uses their normalised 256d Matryoshka prefix.
    pub fn finalize(&mut self) -> Result<()> {
        ensure!(
            matches!(self.stage()?, Stage::Complete | Stage::Ready),
            "ingestion is incomplete"
        );
        if self.stage()? == Stage::Ready {
            return Ok(());
        }
        let read = self.db.begin_read()?;
        let vectors = read.open_table(VECTORS)?;
        let count = vectors.len()?;
        let settings = VectorIndex {
            dimensions: 256,
            clusters: ((count as f64).sqrt() as u32 / 8).clamp(1, 256),
            subvectors: 32,
            retain_raw_for_rerank: true,
        };
        if count != 0 {
            // A bounded, evenly spaced training sample; all vectors are indexed below.
            let stride = (count / 8192).max(1) as usize;
            let sample = vectors
                .iter()?
                .step_by(stride)
                .take(8192)
                .map(|row| {
                    let (id, v) = row?;
                    Ok((id.value(), reduce(&v.value())))
                })
                .collect::<Result<Vec<_>>>()?;
            let tx = self.db.begin_write()?;
            {
                let mut index = tx.open_ivfpq_index(&settings.definition()?)?;
                index.train(sample.into_iter(), 10)?;
                for row in vectors.iter()? {
                    let (id, v) = row?;
                    index.insert(id.value(), &reduce(&v.value()))?;
                }
                index.flush()?;
            }
            tx.open_table(META)?
                .insert("vector_index", serde_json::to_vec(&settings)?.as_slice())?;
            tx.commit()?;
        }
        drop(vectors);
        drop(read);
        self.db.compact()?;
        self.set_stage(Stage::Ready)
    }
}
fn reduce(vector: &[f32; DIMENSIONS]) -> Vec<f32> {
    let mut v = vector[..256].to_vec();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0. {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Copy records and raw vectors only. Resource IDs and payload object names are
/// remapped locally; logical resource identities and article-local anchors stay put.
pub fn merge(inputs: &[std::path::PathBuf], output: &Path) -> Result<Package> {
    ensure!(!inputs.is_empty(), "merge requires at least one input");
    let mut output_package = None;
    for path in inputs {
        let source = ReadOnlyDatabase::open(path)?;
        let read = source.begin_read()?;
        let reader = PackageReader::open(&read)?;
        let progress: Progress = serde_json::from_slice(
            read.open_table(META)?
                .get("progress")?
                .context("missing progress")?
                .value(),
        )?;
        ensure!(
            matches!(progress.stage, Stage::Complete | Stage::Ready),
            "input ingestion is incomplete: {}",
            path.display()
        );
        let profile = reader.embedding_profile()?;
        let package = match &output_package {
            Some(p) => p,
            None => output_package.insert(Package::create(output, &profile)?),
        };
        ensure!(
            package.profile()? == profile,
            "inputs use different embedding profiles"
        );
        let mut remap = std::collections::BTreeMap::new();
        for row in read.open_table(RESOURCES)?.iter()? {
            let (id, value) = row?;
            let resource: Resource = serde_json::from_slice(value.value())?;
            let new = package.next_document()?;
            let body = reader.payload(DocumentId(id.value()))?;
            // Adapt the range reader to std::io::Read without retaining the payload.
            struct Payload<'a>(shodh_redb::BlobReader<'a>);
            impl Read for Payload<'_> {
                fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                    self.0.read_into(bytes).map_err(std::io::Error::other)
                }
            }
            package.add_resource(resource, Payload(body), &[])?;
            remap.insert(id.value(), new);
        }
        let passages = read.open_table(PASSAGES)?;
        let vectors = read.open_table(VECTORS)?;
        ensure!(
            passages.len()? == vectors.len()?,
            "input has missing vectors"
        );
        let mut rows = passages.iter()?;
        loop {
            let tx = package.db.begin_write()?;
            let mut copied = 0;
            {
                let mut target = tx.open_table(PASSAGES)?;
                let mut target_vectors = tx.open_table(VECTORS)?;
                for (next, row) in (target.len()?..).zip(rows.by_ref().take(256)) {
                    let (id, value) = row?;
                    let mut entry: SearchEntry = serde_json::from_slice(value.value())?;
                    entry.resource = *remap
                        .get(&entry.resource.0)
                        .context("missing source resource")?;
                    let vector = vectors
                        .get(id.value())?
                        .context("missing source vector")?
                        .value();
                    target.insert(next, serde_json::to_vec(&entry)?.as_slice())?;
                    target_vectors.insert(next, vector)?;
                    copied += 1;
                }
            }
            if copied == 0 {
                break;
            }
            tx.commit()?;
        }
    }
    let package = output_package.context("no input packages")?;
    package.set_stage(Stage::Complete)?;
    Ok(package)
}
