//! Linux bindings. Files are pinned per request; reads and sockets use Tokio's
//! `embedded-io-async` adapters. Sources must publish by replacement, not mutate
//! a file while it is being served. This is not a sandbox for hostile publishers.
#![forbid(unsafe_code)]

use std::{cell::RefCell, fs::File, io, path::Path, rc::Rc, time::Duration};

use embedded_io_adapters::tokio_1::FromTokio;
use embedded_io_async::{Read, Seek, SeekFrom};
use nanofish::Error;
use omnipotent_serve::{FileStat, FileStore, file_uri_path};
use rustix::fs::{Mode, OFlags, ResolveFlags, open, openat2};
use tokio::{net::TcpStream, sync::Mutex};

mod bundle;

#[derive(Clone)]
enum Source {
    Directory(FileMount),
    Bundle(bundle::BundleMount),
    Sample,
}

/// One local source: a scoped native directory or an immutable article bundle.
/// Bundle paths are `/resources/<bundle-local-u32>` (article record)
/// and `/resources/<package-local-u32>/metadata` (shared Resource metadata).
#[derive(Clone)]
pub struct SourceMount(Source);
impl SourceMount {
    /// The same flash-friendly OPDS 2.0 catalog and EPUB used on embedded hosts.
    pub fn sample() -> Self {
        Self(Source::Sample)
    }

    pub fn from_uri(uri: &str) -> io::Result<Self> {
        let mut buffer = [0; 4096];
        let path = file_uri_path(uri, &mut buffer)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        Ok(Self(if std::fs::metadata(path)?.is_dir() {
            Source::Directory(FileMount::from_uri(uri)?)
        } else {
            Source::Bundle(bundle::BundleMount::open(Path::new(path))?)
        }))
    }
    pub async fn serve_socket(&self, socket: TcpStream, deadline: Duration) -> io::Result<()> {
        match &self.0 {
            Source::Directory(mount) => serve_socket(socket, mount, deadline).await,
            Source::Bundle(mount) => serve_files(socket, &mount.files(), deadline).await,
            Source::Sample => serve_files(socket, &omnipotent_serve::sample::FILES, deadline).await,
        }
    }
}

/// Read-only directory capability. The configured root may itself be a symlink;
/// after open, replacing that symlink cannot redirect this mount. Linux 5.6+
/// openat2 is required; there is deliberately no weaker path-resolution fallback.
#[derive(Clone)]
pub struct FileMount(Rc<File>);

impl FileMount {
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        let mut buffer = [0; 4096];
        let path = file_uri_path(uri, &mut buffer)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let file = File::from(open(
            Path::new(path),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        // Probe the confinement primitive at startup so unsupported kernels fail
        // explicitly rather than quietly presenting every resource as a 404.
        openat2(
            &file,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
        )?;
        Ok(Self(Rc::new(file)))
    }

    /// A fresh view pins at most one file for a single request.
    pub fn files(&self) -> MountedFiles {
        MountedFiles {
            root: self.clone(),
            opened: RefCell::new(None),
        }
    }
}

struct OpenedFile {
    path: String,
    len: usize,
    reader: Rc<Mutex<FileReader>>,
}

struct FileReader {
    io: FromTokio<tokio::fs::File>,
    position: usize,
}

/// A request-scoped file view. The same descriptor supplies stat and all reads,
/// so replacing/unlinking its pathname does not mix generations mid-response.
/// Metadata opens are synchronous (nanofish's stat contract); body I/O is async.
/// Filesystem access failures are intentionally indistinguishable from missing
/// files at the HTTP boundary. Read failures abort an already-started response.
pub struct MountedFiles {
    root: FileMount,
    opened: RefCell<Option<OpenedFile>>,
}

fn safe_relative(path: &str) -> Option<&str> {
    let relative = path.strip_prefix('/')?;
    if relative.is_empty()
        || path.bytes().any(|b| b == b'\\' || b < 0x20 || b == 0x7f)
        || relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    Some(relative)
}

fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    match extension.to_ascii_lowercase().as_str() {
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "zst" | "zstd" => "application/zstd",
        _ => nanofish::mime_for_extension(extension),
    }
}

impl FileStore for MountedFiles {
    fn stat(&self, path: &str) -> Option<FileStat<'_>> {
        let relative = safe_relative(path)?;
        if let Some(file) = self.opened.borrow().as_ref().filter(|f| f.path == path) {
            return Some(FileStat {
                len: file.len,
                etag: None,
                content_type: Some(content_type(path)),
            });
        }
        // NONBLOCK prevents opening a FIFO from stalling the runtime before the
        // regular-file check. BENEATH is enforced atomically by the kernel,
        // including symlink traversal; canonicalize-then-open would be racy.
        let fd = openat2(
            self.root.0.as_ref(),
            relative,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
        )
        .ok()?;
        let file = File::from(fd);
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() {
            return None;
        }
        let len = usize::try_from(metadata.len()).ok()?;
        let reader = FromTokio::new(tokio::fs::File::from_std(file));
        *self.opened.borrow_mut() = Some(OpenedFile {
            path: path.to_owned(),
            len,
            reader: Rc::new(Mutex::new(FileReader {
                io: reader,
                position: 0,
            })),
        });
        Some(FileStat {
            len,
            etag: None,
            content_type: Some(content_type(path)),
        })
    }

    fn stat_gzip(&self, _path: &str) -> Option<FileStat<'_>> {
        None // Native bytes only. No implicit compression/representation variant.
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
        let reader = self
            .opened
            .borrow()
            .as_ref()
            .filter(|f| f.path == path)
            .map(|f| Rc::clone(&f.reader))
            .ok_or(Error::FileError("File not opened"))?;
        let mut reader = reader.lock().await;
        if reader.position != offset {
            reader
                .io
                .seek(SeekFrom::Start(offset as u64))
                .await
                .map_err(|_| Error::FileError("Seek failed"))?;
            reader.position = offset;
        }
        let count = reader
            .io
            .read(out)
            .await
            .map_err(|_| Error::FileError("Read failed"))?;
        reader.position = reader
            .position
            .checked_add(count)
            .ok_or(Error::FileError("Offset overflow"))?;
        Ok(count)
    }
}

/// Adapt an accepted Linux socket, serving one request with a total deadline.
/// Cancellation drops the socket and the request-scoped file handle.
pub async fn serve_socket(
    socket: TcpStream,
    mount: &FileMount,
    deadline: Duration,
) -> io::Result<()> {
    serve_files(socket, &mount.files(), deadline).await
}

async fn serve_files<F: FileStore>(
    socket: TcpStream,
    files: &F,
    deadline: Duration,
) -> io::Result<()> {
    let mut socket = FromTokio::new(socket);
    tokio::time::timeout(
        deadline,
        omnipotent_serve::serve_connection(&mut socket, files),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connection deadline exceeded"))?
    .map_err(|e| io::Error::other(format!("HTTP connection: {e:?}")))
}
