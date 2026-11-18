//! The test harness uses std; the API is exercised with dependency std on/off.
use shodh_redb::{
    BackendError, Builder, ContentType, ReadableDatabase, StorageBackend, StoreOptions,
    TableDefinition, backends::InMemoryBackend,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Debug, Default)]
struct Backend {
    inner: Arc<InMemoryBackend>,
    readonly: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    fail_read: Arc<AtomicBool>,
}
fn denied() -> BackendError {
    #[cfg(feature = "std")]
    {
        BackendError::Io(std::io::Error::other("injected backend failure"))
    }
    #[cfg(not(feature = "std"))]
    {
        BackendError::Message("injected backend failure".into())
    }
}
impl StorageBackend for Backend {
    fn len(&self) -> Result<u64, BackendError> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), BackendError> {
        if self.fail_read.load(Ordering::Relaxed) {
            return Err(denied());
        }
        self.inner.read(offset, out)
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), BackendError> {
        if self.readonly.load(Ordering::Relaxed) {
            self.writes.fetch_add(1, Ordering::Relaxed);
            return Err(denied());
        }
        self.inner.write(offset, data)
    }
    fn set_len(&self, len: u64) -> Result<(), BackendError> {
        if self.readonly.load(Ordering::Relaxed) {
            self.writes.fetch_add(1, Ordering::Relaxed);
            return Err(denied());
        }
        self.inner.set_len(len)
    }
    fn sync_data(&self) -> Result<(), BackendError> {
        Ok(())
    }
}
const TABLE: TableDefinition<u32, u32> = TableDefinition::new("test");
#[test]
fn clean_database_and_blob_open_without_any_write_or_resize() {
    let backend = Backend::default();
    let id;
    {
        let db = Builder::new().create_with_backend(backend.clone()).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(TABLE).unwrap().insert(7, 42).unwrap();
        id = txn
            .store_blob(
                b"prepared native bytes",
                ContentType::OctetStream,
                "article",
                StoreOptions::default(),
            )
            .unwrap();
        txn.commit().unwrap();
    }
    backend.readonly.store(true, Ordering::Relaxed);
    let len = backend.len().unwrap();
    {
        let db = Builder::new()
            .open_read_only_with_backend(backend.clone())
            .unwrap();
        let txn = db.begin_read().unwrap();
        assert_eq!(
            txn.open_table(TABLE)
                .unwrap()
                .get(7)
                .unwrap()
                .unwrap()
                .value(),
            42
        );
        let mut reader = txn.blob_reader(&id).unwrap().unwrap();
        let mut bytes = [0; 64];
        let n = reader.read_into(&mut bytes).unwrap();
        assert_eq!(&bytes[..n], b"prepared native bytes");
    }
    assert_eq!(backend.writes.load(Ordering::Relaxed), 0);
    assert_eq!(backend.len().unwrap(), len);
}
#[test]
fn empty_corrupt_and_failed_backends_are_not_initialized_or_repaired() {
    for corrupt in [false, true] {
        let backend = Backend::default();
        if corrupt {
            backend.set_len(8192).unwrap();
            backend.write(0, &[0xa5; 8192]).unwrap();
        }
        backend.readonly.store(true, Ordering::Relaxed);
        let len = backend.len().unwrap();
        assert!(
            Builder::new()
                .open_read_only_with_backend(backend.clone())
                .is_err()
        );
        assert_eq!(backend.writes.load(Ordering::Relaxed), 0);
        assert_eq!(backend.len().unwrap(), len);
    }
    let backend = Backend::default();
    {
        let _db = Builder::new().create_with_backend(backend.clone()).unwrap();
    }
    backend.readonly.store(true, Ordering::Relaxed);
    backend.fail_read.store(true, Ordering::Relaxed);
    assert!(
        Builder::new()
            .open_read_only_with_backend(backend.clone())
            .is_err()
    );
    assert_eq!(backend.writes.load(Ordering::Relaxed), 0);
}
