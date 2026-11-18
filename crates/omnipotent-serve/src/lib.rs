//! Bounded HTTP serving, independent of filesystems, sockets, and executors.
//!
//! Platforms supply a [`FileStore`] and an `embedded_io_async` stream. A file's
//! declared representation is sent unchanged; neither database records nor a
//! universal content codec are part of this interface. The source must keep
//! each opened representation immutable for the lifetime of the request.
#![no_std]
#![forbid(unsafe_code)]

use embedded_io_async::{Read, Write};
use nanofish::{
    BodyLength, BodyReader, Error, HttpHeader, HttpRequest, ResponseWriter, StatusCode,
    StreamingHandler, handle_http_stream_connection_with_sizes, serve_with,
};
pub use nanofish::{FileStat, FileStore};

pub mod sample;

/// Maximum file bytes read into the serving buffer at once.
pub const STREAM_BUFFER_SIZE: usize = 8192;

struct Handler<'a, F>(&'a F);

impl<F: FileStore> StreamingHandler for Handler<'_, F> {
    async fn handle<S: Read + Write, const HEAD_SIZE: usize>(
        &mut self,
        request: &HttpRequest<'_>,
        _body: &mut BodyReader<'_>,
        response: &mut ResponseWriter<'_, S, HEAD_SIZE>,
    ) -> Result<(), Error> {
        let mut buffer = [0; STREAM_BUFFER_SIZE];
        let extra = [HttpHeader::new("X-Content-Type-Options", "nosniff")];
        if serve_with(self.0, request, response, &mut buffer, &extra).await? {
            return Ok(());
        }
        let message = b"Not found\n";
        response
            .send_head(
                StatusCode::NotFound,
                &[HttpHeader::content_type("text/plain; charset=utf-8")],
                BodyLength::Known(message.len()),
            )
            .await?;
        response.write(message).await
    }
}

/// Serve one request, then close the connection in the platform wrapper.
///
/// Request/head buffers are bounded to 4096/1024 bytes. File bodies stream with
/// backpressure through [`STREAM_BUFFER_SIZE`] bytes. GET/HEAD, directory index
/// lookup and conditional requests are provided by nanofish. No compression is
/// generated here; a store should expose already-decoded native bytes, or an
/// explicitly named compressed file with its own content type.
///
/// # Errors
/// Returns parsing, transport, storage, or response-framing errors. After the
/// response head has been sent, the wrapper must close on error, not append a
/// second response. Dropping this future cancels the request.
pub async fn serve_connection<S: Read + Write, F: FileStore>(
    stream: &mut S,
    files: &F,
) -> Result<(), Error> {
    handle_http_stream_connection_with_sizes::<_, _, 4096, 1024>(stream, &mut Handler(files)).await
}

/// Invalid or unsupported local source URI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceUriError;

impl core::fmt::Display for SourceUriError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("expected an absolute local file:// URI without query, fragment, or NUL")
    }
}

impl core::error::Error for SourceUriError {}

/// Decode a local `file:///absolute/path` (or `file://localhost/path`) into a
/// caller buffer. No filesystem is accessed. Remote authorities are rejected.
pub fn file_uri_path<'a>(uri: &str, buffer: &'a mut [u8]) -> Result<&'a str, SourceUriError> {
    let rest = uri.strip_prefix("file://").ok_or(SourceUriError)?;
    let path = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/').ok_or(SourceUriError)?;
        if !rest[..slash].eq_ignore_ascii_case("localhost") {
            return Err(SourceUriError);
        }
        &rest[slash..]
    };
    if path.contains(['?', '#']) {
        return Err(SourceUriError);
    }
    let decoded = nanofish::percent_decode(path, buffer).map_err(|_| SourceUriError)?;
    if decoded.as_bytes().contains(&0) {
        return Err(SourceUriError);
    }
    Ok(decoded)
}
