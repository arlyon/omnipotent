use super::*;
use omnipotent_format::{
    Provenance, ResourceId, ResourceKey, SourceId, Text, bundle::Bundle, package::SearchEntry,
};

fn profile() -> EmbeddingProfile {
    EmbeddingProfile {
        model: "fixture".into(),
        files: Default::default(),
        dimensions: DIMENSIONS,
        document_template: String::new(),
        query_template: String::new(),
    }
}

fn worker(path: &Path, key: &str, axis: usize) -> Result<()> {
    let mut package = Package::create(path, &profile())?;
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
    let entry = SearchEntry {
        resource: DocumentId(0),
        location: SearchLocation::Article {
            node: None,
            start: 0,
            end: 11,
        },
        heading: String::new(),
        text: "Sample text".into(),
    };
    package.add_article(
        &article,
        &[
            PreparedPassage {
                entry: entry.clone(),
                tokens: vec![1, 2],
            },
            PreparedPassage {
                entry,
                tokens: vec![1, 3],
            },
        ],
    )?;
    package.finish_preparation()?;
    assert!(package.finish_embedding().is_err());
    let mut vector = [0.; DIMENSIONS];
    vector[axis] = 2.;
    vector[DIMENSIONS - 1] = 3.; // This component must not survive finalisation.
    package.store_embeddings([(0, vector)])?;
    drop(package);
    let mut package = Package::open(path)?;
    assert_eq!(package.stage(), Stage::Embedding);
    assert!(!package.tokens.contains_key(0u64.to_be_bytes())?);
    assert!(package.vectors.contains_key(0u64.to_be_bytes())?);
    assert!(package.tokens.contains_key(1u64.to_be_bytes())?);
    package.store_embeddings([(1, vector)])?;
    package.finish_embedding()
}

#[test]
fn merge_remaps_and_exports_searchable_bundle_without_model() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let a = dir.path().join("a.worker");
    let b = dir.path().join("b.worker");
    worker(&a, "a", 0)?;
    worker(&b, "b", 1)?;
    let merged = merge(&[a, b], &dir.path().join("merged.worker"))?;
    assert_eq!(merged.stage(), Stage::Complete);
    let path = dir.path().join("library.bin");
    merged.finalize(&path)?;
    assert!(merged.finalize(&path).is_err());
    let bytes = fs::read(&path)?;
    let bundle = Bundle::parse(&bytes)?;
    assert_eq!(bundle.article_count(), 2);
    assert_eq!(bundle.vectors().len(), 4);
    assert_eq!(bundle.codes().len(), 4);
    let mut query = [0.; 256];
    query[1] = 1.;
    let hits = bundle.search(&query, 1, 4, 1)?;
    assert_eq!(hits[0].score, 1.);
    let loc = &bundle.passages()[hits[0].row as usize];
    assert_eq!(loc.resource(), DocumentId(1));
    let record = bundle.article(loc.resource())?;
    assert_eq!(record.title()?, "b");
    assert_eq!(
        record.excerpt(loc.node(), loc.start(), loc.end())?,
        "Sample text"
    );
    Ok(())
}

#[test]
fn refuses_overwrite_overlap_and_incomplete_ingestion() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("a.worker");
    worker(&path, "same", 0)?;
    assert!(Package::create(&path, &profile()).is_err());
    assert!(merge(&[path.clone(), path], &dir.path().join("overlap.worker")).is_err());
    let empty = Package::create(&dir.path().join("empty.worker"), &profile())?;
    assert!(empty.finalize(&dir.path().join("incomplete.bin")).is_err());
    Ok(())
}
