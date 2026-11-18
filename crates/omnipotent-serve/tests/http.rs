use core::convert::Infallible;
use embedded_io_async::{ErrorType, Read, Write};
use omnipotent_serve::{FileStat, FileStore, STREAM_BUFFER_SIZE, file_uri_path, serve_connection};
use std::cell::Cell;

struct Stream {
    input: Vec<u8>,
    position: usize,
    output: Vec<u8>,
}
impl ErrorType for Stream {
    type Error = Infallible;
}
impl Read for Stream {
    async fn read(&mut self, out: &mut [u8]) -> Result<usize, Infallible> {
        let n = out.len().min(7).min(self.input.len() - self.position);
        out[..n].copy_from_slice(&self.input[self.position..self.position + n]);
        self.position += n;
        Ok(n)
    }
}
impl Write for Stream {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Infallible> {
        let n = bytes.len().min(11); // Exercise backpressure/partial writes.
        self.output.extend_from_slice(&bytes[..n]);
        Ok(n)
    }
    async fn flush(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}
struct Store {
    body: Vec<u8>,
    largest: Cell<usize>,
    reads: Cell<usize>,
    fail: bool,
}
impl FileStore for Store {
    fn stat(&self, path: &str) -> Option<FileStat<'_>> {
        (path == "/article.html").then_some(FileStat {
            len: self.body.len(),
            etag: Some("\"fixture\""),
            content_type: Some("text/html"),
        })
    }
    fn stat_gzip(&self, _: &str) -> Option<FileStat<'_>> {
        None
    }
    async fn read(
        &self,
        _: &str,
        _: bool,
        offset: usize,
        out: &mut [u8],
    ) -> Result<usize, nanofish::Error> {
        self.largest.set(self.largest.get().max(out.len()));
        self.reads.set(self.reads.get() + 1);
        if self.fail {
            return Err(nanofish::Error::FileError("injected failure"));
        }
        let n = out.len().min(self.body.len() - offset);
        out[..n].copy_from_slice(&self.body[offset..offset + n]);
        Ok(n)
    }
}
fn store() -> Store {
    Store {
        body: (0..150_003).map(|n| (n % 251) as u8).collect(),
        largest: Cell::new(0),
        reads: Cell::new(0),
        fail: false,
    }
}
fn exchange<F: FileStore>(request: &str, store: &F) -> (Result<(), nanofish::Error>, Vec<u8>) {
    let mut io = Stream {
        input: request.as_bytes().to_vec(),
        position: 0,
        output: Vec::new(),
    };
    let result = futures_lite::future::block_on(serve_connection(&mut io, store));
    (result, io.output)
}
fn split(response: &[u8]) -> (&str, &[u8]) {
    let end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (
        std::str::from_utf8(&response[..end]).unwrap(),
        &response[end + 4..],
    )
}
#[test]
fn streams_native_bytes_with_partial_io_and_bounded_buffers() {
    let store = store();
    let (result, response) = exchange(
        "GET /article.html HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip, zstd\r\n\r\n",
        &store,
    );
    result.unwrap();
    let (head, body) = split(&response);
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: text/html")
    );
    assert!(!head.to_ascii_lowercase().contains("content-encoding:"));
    assert_eq!(body, store.body);
    assert_eq!(store.largest.get(), STREAM_BUFFER_SIZE);
    assert!(store.reads.get() > 1);
}
#[test]
fn head_and_conditional_get_do_not_read_body() {
    for (method, extra, status) in [
        ("HEAD", "", "200"),
        ("GET", "If-None-Match: \"fixture\"\r\n", "304"),
    ] {
        let store = store();
        let (result, response) = exchange(
            &format!("{method} /article.html HTTP/1.1\r\nHost: localhost\r\n{extra}\r\n"),
            &store,
        );
        result.unwrap();
        let (head, body) = split(&response);
        assert!(head.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(body.is_empty());
        assert_eq!(store.reads.get(), 0);
    }
}
#[test]
fn missing_unsafe_and_invalid_methods() {
    for path in [
        "/missing",
        "/../article.html",
        "/%2e%2e/article.html",
        "/a%2f..%2farticle.html",
    ] {
        let (result, response) = exchange(
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
            &store(),
        );
        result.unwrap();
        assert!(split(&response).0.starts_with("HTTP/1.1 404"));
    }
    let (result, response) = exchange(
        "POST /article.html HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        &store(),
    );
    result.unwrap();
    assert!(split(&response).0.starts_with("HTTP/1.1 405"));
}
#[test]
fn read_failure_aborts_without_appending_a_second_response() {
    let mut store = store();
    store.fail = true;
    let (result, response) = exchange(
        "GET /article.html HTTP/1.1\r\nHost: localhost\r\n\r\n",
        &store,
    );
    assert!(result.is_err());
    let (head, body) = split(&response);
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(body.is_empty());
    assert_eq!(head.matches("HTTP/1.1 ").count(), 1);
}
#[test]
fn local_file_uri_decoding_and_rejection() {
    let mut buf = [0; 128];
    assert_eq!(
        file_uri_path("file:///data/wiki%20en", &mut buf).unwrap(),
        "/data/wiki en"
    );
    assert_eq!(
        file_uri_path("file://localhost/data", &mut buf).unwrap(),
        "/data"
    );
    for uri in [
        "https://host/path",
        "file://remote/data",
        "file://relative",
        "file:///data?x",
        "file:///data#x",
        "file:///a%00b",
        "file:///a%ZZ",
    ] {
        assert!(file_uri_path(uri, &mut buf).is_err(), "{uri}");
    }
    assert!(file_uri_path("file:///long", &mut [0; 2]).is_err());
}

#[test]
fn flash_sample_catalog_and_epub_use_the_same_portable_streaming_path() {
    use omnipotent_serve::sample;
    for path in ["/", "/opds"] {
        let (result, response) = exchange(
            &format!(
                "GET {path} HTTP/1.1\r\nHost: device\r\nAccept: application/opds+json\r\n\r\n"
            ),
            &sample::FILES,
        );
        result.unwrap();
        let (head, body) = split(&response);
        assert!(head.starts_with("HTTP/1.1 200"));
        assert!(
            head.to_ascii_lowercase()
                .contains("content-type: application/opds+json")
        );
        assert_eq!(body, sample::CATALOG);
    }
    let (result, response) = exchange(
        "GET /books/connection-check.epub HTTP/1.1\r\nHost: device\r\nAccept-Encoding: zstd, gzip\r\n\r\n",
        &sample::FILES,
    );
    result.unwrap();
    let (head, body) = split(&response);
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/epub+zip")
    );
    assert!(!head.to_ascii_lowercase().contains("content-encoding:"));
    assert!(sample::BOOK.len() > STREAM_BUFFER_SIZE);
    assert_eq!(body, sample::BOOK);
    let (result, response) = exchange(
        "HEAD /books/connection-check.epub HTTP/1.1\r\nHost: device\r\n\r\n",
        &sample::FILES,
    );
    result.unwrap();
    assert!(split(&response).1.is_empty());
}
