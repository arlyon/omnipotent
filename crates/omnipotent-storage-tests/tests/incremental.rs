//! The same tests run against std and no_std + alloc library builds.
//! This is the existing owned, byte-table delta API, not a streaming sync protocol.
use sha2::{Digest, Sha256};
use shodh_redb::backends::InMemoryBackend;
use shodh_redb::{Builder, Database, IncrementalSnapshot, ReadableDatabase, TableDefinition};

const ITEMS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("items");
const GONE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("gone");

fn database() -> Database {
    Builder::new()
        .set_cache_size(512 * 1024)
        .set_history_retention(16)
        .create_with_backend(InMemoryBackend::new())
        .unwrap()
}

fn seed(db: &Database) {
    let txn = db.begin_write().unwrap();
    {
        let mut table = txn.open_table(ITEMS).unwrap();
        table
            .insert(b"keep".as_slice(), b"same".as_slice())
            .unwrap();
        table
            .insert(b"change".as_slice(), b"old".as_slice())
            .unwrap();
        table
            .insert(b"remove".as_slice(), b"old".as_slice())
            .unwrap();
    }
    txn.open_table(GONE).unwrap();
    txn.commit().unwrap();
}

fn checkpoint(db: &Database) -> u64 {
    db.begin_write().unwrap().commit().unwrap();
    db.transaction_history()
        .unwrap()
        .last()
        .unwrap()
        .transaction_id
}

fn fixture() -> (Database, u64, IncrementalSnapshot) {
    let db = database();
    seed(&db);
    let base = checkpoint(&db);
    let txn = db.begin_write().unwrap();
    {
        let mut table = txn.open_table(ITEMS).unwrap();
        table
            .insert(b"change".as_slice(), b"new".as_slice())
            .unwrap();
        table
            .insert(b"add".as_slice(), b"added".as_slice())
            .unwrap();
        table.remove(b"remove".as_slice()).unwrap();
    }
    txn.delete_table(GONE).unwrap();
    txn.commit().unwrap();
    let delta = db.export_incremental(base).unwrap();
    (db, base, delta)
}

#[test]
fn add_update_delete_drop_roundtrip_without_filesystem() {
    let (_source, base, delta) = fixture();
    assert_eq!(delta.base_txn, base);
    assert_eq!(delta.tables_changed(), 1);
    assert_eq!(delta.total_upserts(), 2);
    assert_eq!(delta.total_deletes(), 1);
    assert_eq!(delta.dropped_table_names(), ["gone"]);
    let decoded = IncrementalSnapshot::from_bytes(&delta.to_bytes()).unwrap();
    let destination = database();
    seed(&destination);
    let old = destination.begin_read().unwrap();
    let report = destination.import_incremental(&decoded).unwrap();
    assert_eq!(report.entries_upserted, 2);
    assert_eq!(report.entries_deleted, 1);
    let current = destination.begin_read().unwrap();
    let table = current.open_table(ITEMS).unwrap();
    assert_eq!(
        table.get(b"keep".as_slice()).unwrap().unwrap().value(),
        b"same"
    );
    assert_eq!(
        table.get(b"change".as_slice()).unwrap().unwrap().value(),
        b"new"
    );
    assert_eq!(
        table.get(b"add".as_slice()).unwrap().unwrap().value(),
        b"added"
    );
    assert!(table.get(b"remove".as_slice()).unwrap().is_none());
    assert!(current.open_table(GONE).is_err());
    assert_eq!(
        old.open_table(ITEMS)
            .unwrap()
            .get(b"change".as_slice())
            .unwrap()
            .unwrap()
            .value(),
        b"old"
    );
}

#[test]
fn unchanged_snapshot_and_missing_history_are_explicit() {
    let db = database();
    seed(&db);
    let base = checkpoint(&db);
    let delta = db.export_incremental(base).unwrap();
    assert_eq!(delta.tables_changed(), 0);
    assert_eq!(delta.total_upserts(), 0);
    assert_eq!(delta.total_deletes(), 0);
    assert!(db.export_incremental(u64::MAX).is_err());
    let bytes = delta.to_bytes();
    assert_eq!(
        IncrementalSnapshot::from_bytes(&bytes)
            .unwrap()
            .tables_changed(),
        0
    );
}

#[test]
fn all_truncations_and_corrupted_bytes_are_rejected() {
    let (_db, _, delta) = fixture();
    let bytes = delta.to_bytes();
    for end in 0..bytes.len() {
        assert!(
            IncrementalSnapshot::from_bytes(&bytes[..end]).is_err(),
            "prefix {end}"
        );
    }
    for index in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[index] ^= 0x80;
        assert!(
            IncrementalSnapshot::from_bytes(&corrupt).is_err(),
            "byte {index}"
        );
    }
}

fn resign(bytes: &mut [u8]) {
    let split = bytes.len() - 32;
    let hash = Sha256::digest(&bytes[..split]);
    bytes[split..].copy_from_slice(&hash);
}

#[test]
fn validly_checksummed_impossible_counts_do_not_preallocate() {
    let (_db, _, delta) = fixture();
    let mut bytes = delta.to_bytes();
    // Wire header is 36 bytes, followed by the first table name and counts.
    let name_len = u16::from_le_bytes(bytes[36..38].try_into().unwrap()) as usize;
    let upsert_count = 38 + name_len;
    bytes[upsert_count..upsert_count + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    resign(&mut bytes);
    assert!(IncrementalSnapshot::from_bytes(&bytes).is_err());

    let db = database();
    seed(&db);
    let base = checkpoint(&db);
    let mut empty = db.export_incremental(base).unwrap().to_bytes();
    empty[36..44].copy_from_slice(&u64::MAX.to_le_bytes());
    resign(&mut empty);
    assert!(IncrementalSnapshot::from_bytes(&empty).is_err());
}

#[test]
fn validly_checksummed_trailing_data_is_rejected() {
    let (_db, _, delta) = fixture();
    let mut bytes = delta.to_bytes();
    bytes.insert(bytes.len() - 32, 0);
    resign(&mut bytes);
    assert!(IncrementalSnapshot::from_bytes(&bytes).is_err());
}

#[test]
fn validly_checksummed_invalid_headers_and_names_are_rejected() {
    let (_db, _, delta) = fixture();
    let original = delta.to_bytes();
    let mut padding = original.clone();
    padding[9] = 1;
    resign(&mut padding);
    assert!(IncrementalSnapshot::from_bytes(&padding).is_err());

    let mut count = original.clone();
    count[28..36].copy_from_slice(&u64::MAX.to_le_bytes());
    resign(&mut count);
    assert!(IncrementalSnapshot::from_bytes(&count).is_err());

    let mut name = original;
    let length = u16::from_le_bytes(name[36..38].try_into().unwrap()) as usize;
    name.drain(38..38 + length);
    name[36..38].fill(0);
    resign(&mut name);
    assert!(IncrementalSnapshot::from_bytes(&name).is_err());
}

#[test]
fn unsupported_existing_typed_table_rolls_back_prior_changes() {
    const TYPED: TableDefinition<u64, u64> = TableDefinition::new("typed");
    let source = database();
    seed(&source);
    let txn = source.begin_write().unwrap();
    txn.open_table(TYPED).unwrap().insert(1, 10).unwrap();
    txn.commit().unwrap();
    let base = checkpoint(&source);
    let txn = source.begin_write().unwrap();
    txn.open_table(ITEMS)
        .unwrap()
        .insert(b"change".as_slice(), b"new".as_slice())
        .unwrap();
    txn.open_table(TYPED).unwrap().insert(1, 20).unwrap();
    txn.commit().unwrap();
    let delta = source.export_incremental(base).unwrap();

    let destination = database();
    seed(&destination);
    let txn = destination.begin_write().unwrap();
    txn.open_table(TYPED).unwrap().insert(1, 10).unwrap();
    txn.commit().unwrap();
    assert!(destination.import_incremental(&delta).is_err());
    let txn = destination.begin_read().unwrap();
    assert_eq!(
        txn.open_table(ITEMS)
            .unwrap()
            .get(b"change".as_slice())
            .unwrap()
            .unwrap()
            .value(),
        b"old"
    );
    assert_eq!(
        txn.open_table(TYPED)
            .unwrap()
            .get(1)
            .unwrap()
            .unwrap()
            .value(),
        10
    );
}
