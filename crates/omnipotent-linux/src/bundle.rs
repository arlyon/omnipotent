//! Linux scheduling for the existing portable mixed-resource package reader.
//! No Article decoding or legacy record framing is performed by this adapter.
use std::{cell::OnceCell, io, sync::Arc};

use nanofish::Error;
use omnipotent_format::{
    DocumentId,
    package::Resource,
    reader::{META, PackageReader},
};
use omnipotent_serve::{FileStat, FileStore, STREAM_BUFFER_SIZE};
use shodh_redb::{Builder, ReadOnlyDatabase, ReadableDatabase};

#[derive(Clone)]
pub(crate) struct BundleMount(Arc<ReadOnlyDatabase>);
impl BundleMount {
    pub(crate) fn open(path: &str) -> io::Result<Self> {
        let db = Builder::new()
            .set_cache_size(2 * 1024 * 1024)
            .open_read_only(path)
            .map_err(io::Error::other)?;
        let txn = db.begin_read().map_err(io::Error::other)?;
        PackageReader::open(&txn).map_err(io::Error::other)?;
        {
            let meta = txn.open_table(META).map_err(io::Error::other)?;
            let progress = meta
                .get("progress")
                .map_err(io::Error::other)?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "bundle has no finalization marker",
                    )
                })?;
            if progress.value().len() > omnipotent_format::package::MAX_METADATA_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "progress metadata exceeds limit",
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(progress.value())?;
            if value.get("ready").and_then(|v| v.as_bool()) != Some(true) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bundle is not finalized",
                ));
            }
        }
        drop(txn);
        Ok(Self(Arc::new(db)))
    }
    pub(crate) fn files(&self) -> BundleFiles {
        BundleFiles {
            db: self.0.clone(),
            entry: OnceCell::new(),
        }
    }
}
struct Entry {
    path: String,
    resource: Resource,
    metadata: Option<Vec<u8>>,
    len: usize,
}
pub(crate) struct BundleFiles {
    db: Arc<ReadOnlyDatabase>,
    entry: OnceCell<Entry>,
}
fn target(path: &str) -> Option<(DocumentId, bool)> {
    let tail = path.strip_prefix("/resources/")?;
    let (number, metadata) = tail
        .strip_suffix("/metadata")
        .map_or((tail, false), |n| (n, true));
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((DocumentId(number.parse().ok()?), metadata))
}
impl FileStore for BundleFiles {
    fn stat(&self, path: &str) -> Option<FileStat<'_>> {
        let (document, metadata) = target(path)?;
        if self.entry.get().is_none() {
            let txn = self.db.begin_read().ok()?;
            let resource = PackageReader::open(&txn).ok()?.resource(document).ok()?;
            let bytes = if metadata {
                Some(serde_json::to_vec(&resource).ok()?)
            } else {
                None
            };
            let len = if let Some(bytes) = &bytes {
                bytes.len()
            } else {
                usize::try_from(resource.payload.length).ok()?
            };
            self.entry
                .set(Entry {
                    path: path.to_owned(),
                    resource,
                    metadata: bytes,
                    len,
                })
                .ok()?;
        }
        let entry = self.entry.get().filter(|e| e.path == path)?;
        Some(FileStat {
            len: entry.len,
            etag: None,
            content_type: Some(if metadata {
                "application/json"
            } else {
                &entry.resource.media_type
            }),
        })
    }
    fn stat_gzip(&self, _: &str) -> Option<FileStat<'_>> {
        None
    }
    async fn read(
        &self,
        path: &str,
        gzip: bool,
        offset: usize,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        if gzip {
            return Err(Error::FileError("No encoded variant"));
        }
        let entry = self
            .entry
            .get()
            .filter(|e| e.path == path)
            .ok_or(Error::FileError("Resource not opened"))?;
        let count = out
            .len()
            .min(STREAM_BUFFER_SIZE)
            .min(entry.len.saturating_sub(offset));
        if let Some(metadata) = &entry.metadata {
            let bytes = metadata
                .get(offset..offset + count)
                .ok_or(Error::FileError("Invalid metadata offset"))?;
            out[..count].copy_from_slice(bytes);
            return Ok(count);
        }
        let db = self.db.clone();
        let object = entry.resource.payload.clone();
        // shodh's backend is synchronous. Keep disk work off the socket executor.
        // One bounded worker buffer is copied into the core's caller-owned buffer;
        // this is streaming, not end-to-end zero-copy. Every transaction reads
        // the same immutable, locked database handle, even across awaits.
        let bytes = tokio::task::spawn_blocking(move || {
            let txn = db
                .begin_read()
                .map_err(|_| Error::FileError("Read transaction failed"))?;
            let package =
                PackageReader::open(&txn).map_err(|_| Error::FileError("Package unavailable"))?;
            let mut reader = package
                .object(&object)
                .map_err(|_| Error::FileError("Object unavailable"))?;
            reader.seek_to(offset as u64);
            let mut bytes = vec![0; count];
            let n = reader
                .read_into(&mut bytes)
                .map_err(|_| Error::FileError("Object read failed"))?;
            bytes.truncate(n);
            Ok::<_, Error>(bytes)
        })
        .await
        .map_err(|_| Error::FileError("Storage worker failed"))??;
        out[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}
