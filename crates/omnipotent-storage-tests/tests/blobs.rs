//! Run with default features and with --no-default-features. The harness uses
//! std, but the database/reader API under test does not require it.
use shodh_redb::backends::InMemoryBackend;
use shodh_redb::{
    BackendError, BlobId, Builder, ContentType, ReadableDatabase, StorageBackend, StoreOptions,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Debug, Default)]
struct MeasuredBackend {
    inner: Arc<InMemoryBackend>,
    bytes: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
}

impl StorageBackend for MeasuredBackend {
    fn len(&self) -> Result<u64, BackendError> {
        self.inner.len()
    }
    fn set_len(&self, len: u64) -> Result<(), BackendError> {
        self.inner.set_len(len)
    }
    fn sync_data(&self) -> Result<(), BackendError> {
        self.inner.sync_data()
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), BackendError> {
        self.inner.write(offset, data)
    }
    fn read(&self, offset: u64, output: &mut [u8]) -> Result<(), BackendError> {
        if self.fail.load(Ordering::Relaxed) {
            #[cfg(feature = "std")]
            return Err(BackendError::Io(std::io::Error::other(
                "injected read failure",
            )));
            #[cfg(not(feature = "std"))]
            return Err(BackendError::Message("injected read failure".into()));
        }
        self.bytes.fetch_add(output.len(), Ordering::Relaxed);
        self.inner.read(offset, output)
    }
}

fn open(backend: MeasuredBackend) -> shodh_redb::Database {
    Builder::new()
        .set_cache_size(512 * 1024)
        .create_with_backend(backend)
        .unwrap()
}

#[test]
fn cold_open_and_small_read_do_not_materialize_large_blob() {
    let backend = MeasuredBackend::default();
    let blob_id;
    {
        let db = open(backend.clone());
        let txn = db.begin_write().unwrap();
        let mut writer = txn
            .blob_writer(ContentType::OctetStream, "large", StoreOptions::default())
            .unwrap();
        let chunk = [0x5a; 64 * 1024];
        for _ in 0..64 {
            writer.write(&chunk).unwrap();
        }
        blob_id = writer.finish().unwrap();
        txn.commit().unwrap();
    }
    let db = open(backend.clone());
    let txn = db.begin_read().unwrap();
    backend.bytes.store(0, Ordering::Relaxed);
    let mut reader = txn.blob_reader(&blob_id).unwrap().unwrap();
    assert_eq!(reader.len(), 4 * 1024 * 1024);
    assert!(
        backend.bytes.load(Ordering::Relaxed) < 512 * 1024,
        "opening read the body"
    );
    let mut buffer = [0; 4096];
    reader.seek_to(64 * 1024 - 17);
    assert_eq!(reader.read_into(&mut buffer).unwrap(), buffer.len());
    assert_eq!(buffer, [0x5a; 4096]);
    assert!(
        backend.bytes.load(Ordering::Relaxed) < 512 * 1024,
        "small range read the body"
    );
}

#[test]
fn caller_buffer_reads_seek_eof_empty_and_uncommitted_data() {
    let db = open(MeasuredBackend::default());
    let txn = db.begin_write().unwrap();
    let data: Vec<u8> = (0..150_003).map(|i| (i % 251) as u8).collect();
    let id = txn
        .store_blob(
            &data,
            ContentType::OctetStream,
            "bytes",
            StoreOptions::default(),
        )
        .unwrap();
    let empty = txn
        .store_blob(
            &[],
            ContentType::OctetStream,
            "empty",
            StoreOptions::default(),
        )
        .unwrap();
    {
        let mut reader = txn.blob_reader(&id).unwrap().unwrap();
        let mut output = [0; 7000];
        let mut position = 0;
        loop {
            let count = reader.read_into(&mut output).unwrap();
            if count == 0 {
                break;
            }
            assert_eq!(&output[..count], &data[position..position + count]);
            position += count;
        }
        assert_eq!(position, data.len());
        assert_eq!(reader.remaining(), 0);
        reader.seek_to(u64::MAX);
        assert_eq!(reader.read_into(&mut output).unwrap(), 0);
        assert!(reader.read_range(u64::MAX, 1).is_err());
        reader.seek_to(2);
        assert_eq!(
            reader.read_range(64 * 1024 - 2, 9).unwrap(),
            data[64 * 1024 - 2..64 * 1024 + 7]
        );
        assert_eq!(reader.position(), 64 * 1024 + 7);
        assert!(reader.read_range(data.len() as u64, 1).is_err());
        assert_eq!(reader.position(), 64 * 1024 + 7);
        assert_eq!(reader.read_range(0, 0).unwrap(), []);
    }
    let mut reader = txn.blob_reader(&empty).unwrap().unwrap();
    assert!(reader.is_empty());
    assert_eq!(reader.read_into(&mut [0; 1]).unwrap(), 0);
    assert!(
        txn.blob_reader(&BlobId::new(u64::MAX, 0))
            .unwrap()
            .is_none()
    );
    txn.commit().unwrap();
}

#[test]
fn old_read_snapshot_survives_blob_deletion_and_dedup_is_readable() {
    let db = open(MeasuredBackend::default());
    let id;
    let duplicate;
    {
        let txn = db.begin_write().unwrap();
        id = txn
            .store_blob(
                b"same bytes",
                ContentType::OctetStream,
                "one",
                StoreOptions::default(),
            )
            .unwrap();
        duplicate = txn
            .store_blob(
                b"same bytes",
                ContentType::OctetStream,
                "two",
                StoreOptions::default(),
            )
            .unwrap();
        txn.commit().unwrap();
    }
    let old = db.begin_read().unwrap();
    let mut reader = old.blob_reader(&id).unwrap().unwrap();
    {
        let txn = db.begin_write().unwrap();
        txn.delete_blob(&id).unwrap();
        txn.commit().unwrap();
    }
    let current = db.begin_read().unwrap();
    assert!(current.blob_reader(&id).unwrap().is_none());
    assert_eq!(reader.read_range(0, 10).unwrap(), b"same bytes");
    assert_eq!(
        current
            .blob_reader(&duplicate)
            .unwrap()
            .unwrap()
            .read_range(0, 10)
            .unwrap(),
        b"same bytes"
    );
}

#[test]
fn storage_error_does_not_advance_cursor() {
    let backend = MeasuredBackend::default();
    let id;
    {
        let db = open(backend.clone());
        let txn = db.begin_write().unwrap();
        let mut writer = txn
            .blob_writer(ContentType::OctetStream, "failure", StoreOptions::default())
            .unwrap();
        for _ in 0..32 {
            writer.write(&[0x77; 64 * 1024]).unwrap();
        }
        id = writer.finish().unwrap();
        txn.commit().unwrap();
    }
    let db = open(backend.clone());
    let txn = db.begin_read().unwrap();
    let mut reader = txn.blob_reader(&id).unwrap().unwrap();
    reader.seek_to(1024 * 1024);
    backend.fail.store(true, Ordering::Relaxed);
    assert!(reader.read_into(&mut [0; 32]).is_err());
    assert_eq!(reader.position(), 1024 * 1024);
    backend.fail.store(false, Ordering::Relaxed);
    let mut output = [0; 32];
    // The engine intentionally poisons the current DB after an I/O error.
    assert!(reader.read_into(&mut output).is_err());
    assert_eq!(reader.position(), 1024 * 1024);
    drop(txn);
    drop(db);
    let db = open(backend);
    let txn = db.begin_read().unwrap();
    let mut reader = txn.blob_reader(&id).unwrap().unwrap();
    reader.seek_to(1024 * 1024);
    assert_eq!(reader.read_into(&mut output).unwrap(), 32);
    assert_eq!(output, [0x77; 32]);
}

#[cfg(feature = "std")]
#[test]
fn standard_read_and_seek_use_lazy_reader() {
    use std::io::{Read, Seek, SeekFrom};
    let db = open(MeasuredBackend::default());
    let txn = db.begin_write().unwrap();
    let id = txn
        .store_blob(
            b"abcdef",
            ContentType::OctetStream,
            "std",
            StoreOptions::default(),
        )
        .unwrap();
    let mut reader = txn.blob_reader(&id).unwrap().unwrap();
    assert_eq!(reader.seek(SeekFrom::End(-3)).unwrap(), 3);
    let mut buffer = [0; 3];
    reader.read_exact(&mut buffer).unwrap();
    assert_eq!(&buffer, b"def");
    assert!(reader.seek(SeekFrom::Current(-7)).is_err());
    assert_eq!(reader.position(), 6);
}
