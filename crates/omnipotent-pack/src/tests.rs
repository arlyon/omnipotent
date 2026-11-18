use super::*;
use omnipotent_format::{Provenance, ResourceId, ResourceKey, SourceId, Text};

fn profile() -> EmbeddingProfile {
    EmbeddingProfile {
        model: "fixture".into(),
        files: Default::default(),
        dimensions: 768,
        document_template: String::new(),
        query_template: String::new(),
    }
}
fn worker(path: &Path, key: &str) -> Result<()> {
    let package = Package::create(path, &profile())?;
    let article = Article {
        id: ResourceId {
            source: SourceId::new("fixture")?,
            key: ResourceKey::new(key)?,
        },
        title: key.into(),
        description: None,
        abstract_text: Some(Text::plain("Sample text")),
        provenance: Provenance {
            origin_uri: None,
            language: None,
            revision: None,
            licenses: vec![],
            extra: Default::default(),
        },
        parts: vec![],
        tables: vec![],
        references: vec![],
        assets: vec![],
        links: vec![],
        extra: Default::default(),
    };
    package.add_article(
        &article,
        &[PreparedPassage {
            entry: SearchEntry {
                resource: DocumentId(0),
                location: SearchLocation::Article {
                    node: None,
                    start: 0,
                    end: 11,
                },
                heading: String::new(),
                text: "Sample text".into(),
            },
            tokens: vec![1, 2],
        }],
    )?;
    package.finish_preparation()?;
    assert!(package.finish_embedding().is_err());
    let tx = package.db.begin_write()?;
    let mut vector = [0.; DIMENSIONS];
    vector[0] = 1.;
    tx.open_table(VECTORS)?.insert(0, vector)?;
    tx.open_table(TOKENS)?.remove(0)?;
    tx.commit()?;
    package.finish_embedding()?;
    Ok(())
}
#[test]
fn merge_remaps_and_finalizes_without_a_model() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let a = dir.path().join("a.redb");
    let b = dir.path().join("b.redb");
    worker(&a, "a")?;
    worker(&b, "b")?;
    let mut merged = merge(&[a, b], &dir.path().join("merged.redb"))?;
    assert_eq!(merged.stage()?, Stage::Complete);
    {
        let tx = merged.db.begin_read()?;
        let reader = PackageReader::open(&tx)?;
        assert_eq!(reader.search_entry(1)?.resource, DocumentId(1));
        assert_eq!(reader.resource(DocumentId(1))?.title, "b");
        assert_eq!(tx.open_table(VECTORS)?.len()?, 2);
    }
    merged.finalize()?;
    merged.finalize()?;
    assert_eq!(merged.stage()?, Stage::Ready);
    let tx = merged.db.begin_read()?;
    let mut query = vec![0.; 256];
    query[0] = 1.;
    let hits = PackageReader::open(&tx)?.search(
        &profile(),
        &query,
        &shodh_redb::SearchParams {
            k: 1,
            nprobe: 1,
            ..shodh_redb::SearchParams::top_k(1)
        },
    )?;
    assert_eq!(hits.len(), 1);
    Ok(())
}
#[test]
fn refuses_overwrite_overlap_and_incomplete_ingestion() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.redb");
    worker(&path, "same")?;
    assert!(Package::create(&path, &profile()).is_err());
    assert!(merge(&[path.clone(), path], &dir.path().join("overlap.redb")).is_err());
    let mut empty = Package::create(&dir.path().join("empty.redb"), &profile())?;
    assert!(empty.finalize().is_err());
    Ok(())
}
