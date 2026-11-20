//! Fjall workers and offline bundle assembly.
mod output;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
use fjall::{
    Database, Keyspace, KeyspaceCreateOptions, OwnedWriteBatch as WriteBatch, PersistMode,
};
use omnipotent_embed::{DIMENSIONS, Encoder, PreparedPassage, batch_len};
use omnipotent_format::{
    Article, DocumentId, Record,
    package::{EmbeddingProfile, MAX_ARTICLE_BYTES, PassageRef, SearchLocation},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use zerocopy::{
    FromBytes, IntoBytes,
    little_endian::{F32, U32},
};

type RawVector = [F32; DIMENSIONS];
const WORKER_MARKER: &str = "omnipotent-worker-v1";
const KEYSPACES: [&str; 6] = [
    "metadata",
    "articles",
    "identities",
    "passages",
    "tokens",
    "vectors",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    Preparing,
    Embedding,
    Complete,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Progress {
    stage: Stage,
    documents: u32,
    passages: u64,
    embedded: u64,
}

pub struct Package {
    db: Database,
    meta: Keyspace,
    articles: Keyspace,
    identities: Keyspace,
    passages: Keyspace,
    tokens: Keyspace,
    vectors: Keyspace,
    progress: Progress,
    profile: EmbeddingProfile,
}

impl Package {
    pub fn create(path: &Path, profile: &EmbeddingProfile) -> Result<Self> {
        fs::create_dir(path).with_context(|| {
            format!(
                "creating new worker {} (will not overwrite)",
                path.display()
            )
        })?;
        let db = Database::builder(path).open()?;
        let progress = Progress {
            stage: Stage::Preparing,
            documents: 0,
            passages: 0,
            embedded: 0,
        };
        let worker = Self::tables(db, progress, profile.clone())?;
        let mut batch = worker.batch();
        batch.insert(&worker.meta, "profile", serde_json::to_vec(profile)?);
        worker.commit(batch, progress)?;
        fs::File::create_new(path.join(WORKER_MARKER))?.sync_all()?;
        Ok(worker)
    }

    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            path.join(WORKER_MARKER).is_file(),
            "expected an Omnipotent worker directory: {}",
            path.display()
        );
        let db = Database::builder(path).open()?;
        ensure!(
            KEYSPACES.iter().all(|name| db.keyspace_exists(name)),
            "not an Omnipotent worker: {}",
            path.display()
        );
        let meta = db.keyspace(KEYSPACES[0], KeyspaceCreateOptions::default)?;
        let profile =
            serde_json::from_slice(&meta.get("profile")?.context("missing worker profile")?)?;
        let progress =
            serde_json::from_slice(&meta.get("progress")?.context("missing worker progress")?)?;
        Self::tables(db, progress, profile)
    }

    fn tables(db: Database, progress: Progress, profile: EmbeddingProfile) -> Result<Self> {
        let [meta, articles, identities, passages, tokens, vectors] = KEYSPACES;
        Ok(Self {
            meta: db.keyspace(meta, KeyspaceCreateOptions::default)?,
            articles: db.keyspace(articles, KeyspaceCreateOptions::default)?,
            identities: db.keyspace(identities, KeyspaceCreateOptions::default)?,
            passages: db.keyspace(passages, KeyspaceCreateOptions::default)?,
            tokens: db.keyspace(tokens, KeyspaceCreateOptions::default)?,
            vectors: db.keyspace(vectors, KeyspaceCreateOptions::default)?,
            db,
            progress,
            profile,
        })
    }

    fn batch(&self) -> WriteBatch {
        self.db.batch().durability(Some(PersistMode::SyncAll))
    }
    fn commit(&self, mut batch: WriteBatch, progress: Progress) -> Result<()> {
        batch.insert(&self.meta, "progress", serde_json::to_vec(&progress)?);
        batch.commit()?;
        Ok(())
    }
    fn advance(&mut self, batch: WriteBatch, progress: Progress) -> Result<()> {
        self.commit(batch, progress)?;
        self.progress = progress;
        Ok(())
    }
    pub fn stage(&self) -> Stage {
        self.progress.stage
    }
    pub fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }
    pub fn next_document(&self) -> DocumentId {
        DocumentId(self.progress.documents)
    }

    fn insert_article(&self, batch: &mut WriteBatch, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_ARTICLE_BYTES,
            "article exceeds package size limit"
        );
        let (source, key) = Record::parse(bytes)?.resource_id()?;
        let identity = serde_json::to_vec(&(source, key))?;
        ensure!(
            !self.identities.contains_key(&identity)?,
            "duplicate resource {source}/{key}"
        );
        batch.insert(
            &self.identities,
            identity,
            self.progress.documents.to_be_bytes(),
        );
        batch.insert(&self.articles, self.progress.documents.to_be_bytes(), bytes);
        Ok(())
    }

    pub fn add_article(&mut self, article: &Article, prepared: &[PreparedPassage]) -> Result<()> {
        ensure!(self.stage() == Stage::Preparing, "preparation is closed");
        let bytes = omnipotent_format::record::encode(article)?;
        let view = Record::parse(&bytes)?;
        let mut batch = self.batch();
        self.insert_article(&mut batch, &bytes)?;
        let mut progress = self.progress;
        progress.documents = progress
            .documents
            .checked_add(1)
            .context("too many articles")?;
        for passage in prepared {
            let SearchLocation::Article { node, start, end } = passage.entry.location else {
                anyhow::bail!("article requires an article passage location");
            };
            ensure!(
                passage.entry.resource == self.next_document()
                    && view.excerpt(node, start, end)? == passage.entry.text,
                "passage location does not match article text"
            );
            batch_len([passage.tokens.len()], omnipotent_embed::MAX_BATCH_SIZE)?;
            let location = PassageRef::new(passage.entry.resource, node, start, end)?;
            let key = progress.passages.to_be_bytes();
            batch.insert(&self.passages, key, location.bytes());
            let tokens: Vec<_> = passage.tokens.iter().copied().map(U32::new).collect();
            batch.insert(&self.tokens, key, tokens.as_bytes());
            progress.passages = progress
                .passages
                .checked_add(1)
                .context("too many passages")?;
        }
        self.advance(batch, progress)
    }

    pub fn finish_preparation(&mut self) -> Result<()> {
        ensure!(self.stage() == Stage::Preparing, "not preparing");
        self.advance(
            self.batch(),
            Progress {
                stage: Stage::Embedding,
                ..self.progress
            },
        )
    }

    fn store_embeddings(
        &mut self,
        rows: impl IntoIterator<Item = (u64, [f32; DIMENSIONS])>,
    ) -> Result<()> {
        let mut batch = self.batch();
        let mut progress = self.progress;
        for (id, vector) in rows {
            let key = id.to_be_bytes();
            ensure!(
                self.tokens.contains_key(key)? && !self.vectors.contains_key(key)?,
                "embedding row is not pending"
            );
            batch.insert(&self.vectors, key, vector.map(F32::new).as_bytes());
            batch.remove(&self.tokens, key);
            progress.embedded += 1;
        }
        self.advance(batch, progress)
    }

    pub fn embed(&mut self, encoder: &Encoder, maximum: usize) -> Result<()> {
        ensure!(
            self.stage() == Stage::Embedding,
            "finish preparation before embedding"
        );
        let mut descriptors = Vec::new();
        for row in self.tokens.iter() {
            let (id, bytes) = row.into_inner()?;
            ensure!(bytes.len() % 4 == 0, "invalid prepared token row");
            descriptors.push((
                u32::try_from(bytes.len() / 4)?,
                u64::from_be_bytes(id.as_ref().try_into()?),
            ));
        }
        eprintln!("sorting {} pending passages", descriptors.len());
        descriptors.sort_unstable();
        let mut cursor = 0;
        while cursor < descriptors.len() {
            let count = batch_len(descriptors[cursor..].iter().map(|d| d.0 as usize), maximum)?;
            let batch = &descriptors[cursor..cursor + count];
            let rows = batch
                .iter()
                .map(|(_, id)| {
                    let bytes = self
                        .tokens
                        .get(id.to_be_bytes())?
                        .context("missing prepared tokens")?;
                    Ok(<[U32]>::ref_from_bytes(&bytes)
                        .map_err(|_| anyhow::anyhow!("invalid token row"))?
                        .iter()
                        .map(|token| token.get())
                        .collect())
                })
                .collect::<Result<Vec<_>>>()?;
            let vectors = encoder.embed(&rows)?;
            ensure!(vectors.len() == count, "unexpected embedding batch size");
            self.store_embeddings(batch.iter().map(|(_, id)| *id).zip(vectors))?;
            cursor += count;
            eprintln!(
                "embedded {}/{}",
                self.progress.embedded, self.progress.passages
            );
        }
        self.finish_embedding()
    }

    pub fn finish_embedding(&mut self) -> Result<()> {
        ensure!(self.stage() == Stage::Embedding, "not embedding");
        ensure!(
            self.progress.embedded == self.progress.passages && self.tokens.is_empty()?,
            "embedding is incomplete"
        );
        self.advance(
            self.batch(),
            Progress {
                stage: Stage::Complete,
                ..self.progress
            },
        )
    }

    pub fn finalize(&self, out: &Path) -> Result<()> {
        ensure!(self.stage() == Stage::Complete, "ingestion is incomplete");
        output::write(self, out)
    }
}

pub fn merge(inputs: &[PathBuf], output: &Path) -> Result<Package> {
    ensure!(!inputs.is_empty(), "merge requires at least one input");
    let mut target: Option<Package> = None;
    for path in inputs {
        let source = Package::open(path)?;
        ensure!(
            source.stage() == Stage::Complete,
            "merge requires a complete worker: {}",
            path.display()
        );
        let target = match target {
            Some(ref mut target) => target,
            None => target.insert(Package::create(output, source.profile())?),
        };
        ensure!(
            source.profile() == target.profile(),
            "embedding profiles differ"
        );
        let document_base = target.progress.documents;
        for row in source.articles.iter() {
            let (key, bytes) = row.into_inner()?;
            let id = u32::from_be_bytes(key.as_ref().try_into()?);
            ensure!(
                id == target.progress.documents - document_base,
                "non-contiguous article keys"
            );
            let mut batch = target.batch();
            target.insert_article(&mut batch, &bytes)?;
            let documents = target
                .progress
                .documents
                .checked_add(1)
                .context("too many articles")?;
            target.advance(
                batch,
                Progress {
                    documents,
                    ..target.progress
                },
            )?;
        }
        let mut passages = source.passages.iter();
        loop {
            let mut batch = target.batch();
            let mut progress = target.progress;
            for row in passages.by_ref().take(256) {
                let (key, payload) = row.into_inner()?;
                let mut passage = *PassageRef::parse(&payload)?;
                ensure!(
                    passage.resource().0 < source.progress.documents,
                    "unknown source document"
                );
                passage.remap(DocumentId(document_base + passage.resource().0));
                let vector = source.vectors.get(&key)?.context("missing source vector")?;
                let key = progress.passages.to_be_bytes();
                batch.insert(&target.passages, key, passage.bytes());
                batch.insert(&target.vectors, key, vector);
                progress.passages += 1;
                progress.embedded += 1;
            }
            if progress.passages == target.progress.passages {
                break;
            }
            target.advance(batch, progress)?;
        }
    }
    let mut target = target.context("missing merge inputs")?;
    target.advance(
        target.batch(),
        Progress {
            stage: Stage::Complete,
            ..target.progress
        },
    )?;
    Ok(target)
}
