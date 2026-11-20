use std::{cell::OnceCell, io, path::Path, sync::Arc};

use nanofish::Error;
use omnipotent_format::{
    DocumentId, LocalId,
    bundle::Bundle,
    package::{ARTICLE_MEDIA_TYPE, ObjectRef, Resource, ResourceKind},
};
use omnipotent_serve::{FileStat, FileStore};

#[derive(Clone)]
pub(crate) struct BundleMount(Arc<[u8]>);
impl BundleMount {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Bundle::parse(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self(bytes.into()))
    }
    pub(crate) fn files(&self) -> BundleFiles<'_> {
        BundleFiles {
            bundle: Bundle::parse(&self.0).expect("validated immutable bundle"),
            entry: OnceCell::new(),
        }
    }
}

pub(crate) struct BundleFiles<'a> {
    bundle: Bundle<'a>,
    entry: OnceCell<Entry<'a>>,
}
struct Entry<'a> {
    path: String,
    record: &'a [u8],
    metadata: Option<Vec<u8>>,
}
impl Entry<'_> {
    fn body(&self) -> &[u8] {
        self.metadata.as_deref().unwrap_or(self.record)
    }
}

impl FileStore for BundleFiles<'_> {
    fn stat(&self, path: &str) -> Option<FileStat<'_>> {
        if self.entry.get().is_none() {
            let suffix = path.strip_prefix("/resources/")?;
            let (id, metadata) = suffix
                .strip_suffix("/metadata")
                .map_or((suffix, false), |id| (id, true));
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let document = DocumentId(id.parse().ok()?);
            let record = self.bundle.article_bytes(document).ok()?;
            let article = omnipotent_format::Record::parse(record).ok()?;
            let metadata = if metadata {
                let article = article.to_owned().ok()?;
                let resource = Resource {
                    id: article.id,
                    title: article.title,
                    provenance: article.provenance,
                    kind: ResourceKind::Article,
                    media_type: ARTICLE_MEDIA_TYPE.into(),
                    payload: ObjectRef {
                        object: LocalId::new(format!("resource/{}/payload", document.0)).ok()?,
                        length: record.len() as u64,
                    },
                };
                Some(serde_json::to_vec(&resource).ok()?)
            } else {
                None
            };
            self.entry
                .set(Entry {
                    path: path.into(),
                    record,
                    metadata,
                })
                .ok()?;
        }
        let entry = self.entry.get().filter(|e| e.path == path)?;
        Some(FileStat {
            len: entry.body().len(),
            etag: None,
            content_type: Some(if entry.metadata.is_some() {
                "application/json"
            } else {
                ARTICLE_MEDIA_TYPE
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
        let body = entry.body();
        let count = out.len().min(body.len().saturating_sub(offset));
        if count != 0 {
            out[..count].copy_from_slice(&body[offset..offset + count]);
        }
        Ok(count)
    }
}
