//! The reader and storage dependency run with std disabled in the canonical
//! --no-default-features invocation. The host harness still uses std for tests.
use omnipotent_format::package::{
    ObjectRef, PACKAGE_VERSION, Resource, ResourceKind, SearchEntry, SearchLocation,
};
use omnipotent_format::reader::{IDENTITIES, META, OBJECTS, PASSAGES, PackageReader, RESOURCES};
use omnipotent_format::{DocumentId, LocalId, Provenance, ResourceId, ResourceKey, SourceId};
use shodh_redb::{Builder, ContentType, ReadableDatabase, StoreOptions, backends::InMemoryBackend};
use std::collections::BTreeMap;

#[test]
fn portable_reader_handles_native_payloads_and_non_article_search_targets() {
    let db = Builder::new()
        .create_with_backend(InMemoryBackend::new())
        .unwrap();
    let bytes = b"%PDF-1.7\nNative bytes, not an Article";
    let id = ResourceId {
        source: SourceId::new("manuals").unwrap(),
        key: ResourceKey::new("repair-guide").unwrap(),
    };
    let resource = Resource {
        id: id.clone(),
        kind: ResourceKind::Publication,
        title: "Repair guide".into(),
        media_type: "application/pdf".into(),
        provenance: Provenance {
            origin_uri: None,
            language: None,
            revision: None,
            licenses: Vec::new(),
            extra: BTreeMap::new(),
        },
        payload: ObjectRef {
            object: LocalId::new("resource/0/payload").unwrap(),
            length: bytes.len() as u64,
        },
    };
    let entry = SearchEntry {
        resource: DocumentId(0),
        location: SearchLocation::Native {
            fragment: Some("page=12".into()),
        },
        heading: "Repair".into(),
        text: "Fix the part".into(),
    };
    let txn = db.begin_write().unwrap();
    txn.open_table(META)
        .unwrap()
        .insert("schema", PACKAGE_VERSION.to_le_bytes().as_slice())
        .unwrap();
    let blob = txn
        .store_blob(
            bytes,
            ContentType::OctetStream,
            "publication",
            StoreOptions::default(),
        )
        .unwrap();
    txn.open_table(OBJECTS)
        .unwrap()
        .insert("resource/0/payload", blob)
        .unwrap();
    txn.open_table(RESOURCES)
        .unwrap()
        .insert(0, serde_json::to_vec(&resource).unwrap().as_slice())
        .unwrap();
    txn.open_table(IDENTITIES)
        .unwrap()
        .insert(serde_json::to_vec(&id).unwrap().as_slice(), 0)
        .unwrap();
    txn.open_table(PASSAGES)
        .unwrap()
        .insert(0, serde_json::to_vec(&entry).unwrap().as_slice())
        .unwrap();
    txn.commit().unwrap();
    let txn = db.begin_read().unwrap();
    let reader = PackageReader::open(&txn).unwrap();
    assert_eq!(reader.find(&id).unwrap(), Some(DocumentId(0)));
    assert_eq!(reader.resource(DocumentId(0)).unwrap(), resource);
    assert_eq!(reader.search_entry(0).unwrap(), entry);
    let mut blob = reader.payload(DocumentId(0)).unwrap();
    let mut output = [0; 8];
    assert_eq!(blob.read_into(&mut output).unwrap(), output.len());
    assert_eq!(&output, &bytes[..8]);
}
