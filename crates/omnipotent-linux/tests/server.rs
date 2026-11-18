use omnipotent_linux::{FileMount, serve_socket};
use omnipotent_serve::FileStore;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    os::unix::fs::symlink,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server {
    child: Child,
    address: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn start(root: &std::path::Path) -> Self {
        Self::launch(&["--source", &format!("file://{}", root.display())])
    }
    fn sample() -> Self {
        Self::launch(&["--sample"])
    }
    fn launch(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_omnipotent-linux"))
            .args(args)
            .args(["--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line
            .strip_prefix("LISTENING http://")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned();
        Self { child, address }
    }
    fn request(&self, method: &str, path: &str) -> Vec<u8> {
        let mut socket = TcpStream::connect(&self.address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: zstd, gzip\r\nConnection: close\r\n\r\n").unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).unwrap();
        bytes
    }
}
fn split(response: &[u8]) -> (&str, &[u8]) {
    let end = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (
        std::str::from_utf8(&response[..end]).unwrap(),
        &response[end + 4..],
    )
}
#[test]
fn binary_serves_prepared_native_article_and_files_over_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    fs::create_dir(&root).unwrap();
    let article = format!(
        "<!doctype html><title>Prepared article</title><article>{}</article>",
        "Portable offline reference — unchanged. ".repeat(10_000)
    );
    // Preparation publishes a complete representation, not a still-growing file.
    fs::write(root.join("article.staging"), article.as_bytes()).unwrap();
    fs::rename(root.join("article.staging"), root.join("article.html")).unwrap();
    fs::write(root.join("index.html"), b"<h1>Library</h1>").unwrap();
    fs::write(root.join("book.pdf"), b"%PDF-1.7\n\0native PDF bytes").unwrap();
    fs::write(root.join("space name.txt"), b"decoded path").unwrap();
    fs::write(root.join("empty.txt"), []).unwrap();
    fs::write(dir.path().join("secret.txt"), b"outside mount").unwrap();
    symlink("article.html", root.join("alias.html")).unwrap();
    symlink(dir.path().join("secret.txt"), root.join("escape.txt")).unwrap();
    fs::create_dir(root.join("folder")).unwrap();
    let server = Server::start(&root);
    let response = server.request("GET", "/article.html");
    let (head, body) = split(&response);
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("content-length: {}", article.len()))
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: text/html")
    );
    assert!(!head.to_ascii_lowercase().contains("content-encoding:"));
    assert_eq!(body, article.as_bytes());
    assert_eq!(
        split(&server.request("GET", "/alias.html")).1,
        article.as_bytes()
    );
    assert_eq!(split(&server.request("GET", "/")).1, b"<h1>Library</h1>");
    assert_eq!(
        split(&server.request("GET", "/space%20name.txt")).1,
        b"decoded path"
    );
    let pdf = server.request("GET", "/book.pdf");
    assert!(
        split(&pdf)
            .0
            .to_ascii_lowercase()
            .contains("content-type: application/pdf")
    );
    assert_eq!(split(&pdf).1, b"%PDF-1.7\n\0native PDF bytes");
    let head_response = server.request("HEAD", "/article.html");
    assert!(split(&head_response).1.is_empty());
    assert!(split(&server.request("GET", "/empty.txt")).1.is_empty());
    for path in [
        "/missing",
        "/escape.txt",
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/folder/../article.html",
        "/folder",
    ] {
        assert!(
            split(&server.request("GET", path))
                .0
                .starts_with("HTTP/1.1 404"),
            "{path}"
        );
    }
    assert!(
        split(&server.request("POST", "/article.html"))
            .0
            .starts_with("HTTP/1.1 405")
    );
    // A stalled connection cannot stop another client on the single-thread executor.
    let mut stalled = TcpStream::connect(&server.address).unwrap();
    stalled.write_all(b"GE").unwrap();
    assert_eq!(split(&server.request("GET", "/")).1, b"<h1>Library</h1>");
}
#[tokio::test]
async fn opened_file_and_mount_survive_atomic_path_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("v1");
    let second = dir.path().join("v2");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    fs::write(first.join("article.txt"), b"old").unwrap();
    fs::write(second.join("article.txt"), b"other root").unwrap();
    let link = dir.path().join("current");
    symlink(&first, &link).unwrap();
    let mount = FileMount::from_uri(&format!("file://{}", link.display())).unwrap();
    let files = mount.files();
    assert_eq!(files.stat("/article.txt").unwrap().len, 3);
    fs::write(first.join("next"), b"new").unwrap();
    fs::rename(first.join("next"), first.join("article.txt")).unwrap();
    let next_link = dir.path().join("next-root");
    symlink(&second, &next_link).unwrap();
    fs::rename(&next_link, &link).unwrap();
    let mut buffer = [0; 32];
    let n = files
        .read("/article.txt", false, 0, &mut buffer)
        .await
        .unwrap();
    assert_eq!(&buffer[..n], b"old");
    let fresh = mount.files();
    fresh.stat("/article.txt").unwrap();
    let n = fresh
        .read("/article.txt", false, 0, &mut buffer)
        .await
        .unwrap();
    assert_eq!(&buffer[..n], b"new"); // Mount descriptor did not follow the changed root symlink.
    assert!(fresh.stat("/../v2/article.txt").is_none());
}
#[tokio::test]
async fn stalled_connection_hits_deadline_and_releases_socket() {
    use tokio::io::AsyncReadExt;
    let dir = tempfile::tempdir().unwrap();
    let mount = FileMount::from_uri(&format!("file://{}", dir.path().display())).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let error = serve_socket(server, &mount, Duration::from_millis(20))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    let n = tokio::time::timeout(Duration::from_secs(1), client.read(&mut [0; 1]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(n, 0);
}
#[test]
fn invalid_sources_fail_explicitly() {
    for uri in [
        "https://example.org/library",
        "file://remote/library",
        "file:///not-a-real-omnipotent-path",
    ] {
        assert!(FileMount::from_uri(uri).is_err());
    }
    let file = tempfile::NamedTempFile::new().unwrap();
    assert!(FileMount::from_uri(&format!("file://{}", file.path().display())).is_err());
}

#[test]
fn finalized_assembler_bundle_streams_article_metadata_and_native_publication() {
    use omnipotent_format::package::{
        ARTICLE_MEDIA_TYPE, EmbeddingProfile, ObjectRef, Resource, ResourceKind,
    };
    use omnipotent_format::reader::{META, PackageReader};
    use omnipotent_format::{
        Article, DocumentId, LocalId, Provenance, ResourceId, ResourceKey, SourceId,
    };
    use omnipotent_pack::Package;
    use shodh_redb::ReadableDatabase;
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.redb");
    let profile = EmbeddingProfile {
        model: "reader-fixture".into(),
        files: BTreeMap::new(),
        dimensions: 256,
        document_template: String::new(),
        query_template: String::new(),
    };
    let mut package = Package::create(&path, &profile).unwrap();
    let provenance = Provenance {
        origin_uri: None,
        language: None,
        revision: None,
        licenses: Vec::new(),
        extra: BTreeMap::new(),
    };
    let title = format!(
        "Prepared Wikipedia fixture {}",
        "native representation ".repeat(500)
    );
    for id in 1..=256 {
        let article = Article {
            id: ResourceId {
                source: SourceId::new("wikipedia:en").unwrap(),
                key: ResourceKey::new(id.to_string()).unwrap(),
            },
            title: title.clone(),
            description: Some("streamed content ".repeat(10_000)),
            abstract_text: None,
            provenance: provenance.clone(),
            parts: Vec::new(),
            tables: Vec::new(),
            references: Vec::new(),
            assets: Vec::new(),
            links: Vec::new(),
            extra: BTreeMap::new(),
        };
        package.add_article(&article, &[]).unwrap();
    }
    let pdf: Vec<u8> = b"%PDF-1.7\nNative publication bytes\n".repeat(10_000);
    let pdf_path = dir.path().join("book.pdf");
    fs::write(&pdf_path, &pdf).unwrap();
    package
        .add_native(
            Resource {
                id: ResourceId {
                    source: SourceId::new("manuals").unwrap(),
                    key: ResourceKey::new("guide").unwrap(),
                },
                kind: ResourceKind::Publication,
                title: "Native publication".into(),
                media_type: "application/pdf".into(),
                provenance,
                payload: ObjectRef {
                    object: LocalId::new("pending").unwrap(),
                    length: 0,
                },
            },
            &pdf_path,
        )
        .unwrap();
    // Live databases cannot be mounted even before the readiness check.
    let uri = format!("file://{}", path.display());
    assert!(omnipotent_linux::SourceMount::from_uri(&uri).is_err());
    package.finish_preparation().unwrap();
    package.finish_embedding().unwrap();
    package.finalize().unwrap();
    let (expected, metadata) = {
        let txn = package.db.begin_read().unwrap();
        let reader = PackageReader::open(&txn).unwrap();
        let resource = reader.resource(DocumentId(0)).unwrap();
        let mut body = reader.payload(DocumentId(0)).unwrap();
        (
            body.read_range(0, body.len() as usize).unwrap(),
            serde_json::to_vec(&resource).unwrap(),
        )
    };
    assert!(expected.len() > omnipotent_serve::STREAM_BUFFER_SIZE);
    drop(package); // Finalized and closed before the source opens it.
    {
        let server = Server::start(&path);
        let response = server.request("GET", "/resources/0");
        assert!(
            split(&response)
                .0
                .to_ascii_lowercase()
                .contains(&format!("content-type: {ARTICLE_MEDIA_TYPE}"))
        );
        assert!(
            !split(&response)
                .0
                .to_ascii_lowercase()
                .contains("content-encoding:")
        );
        assert_eq!(split(&response).1, expected);
        let decoded: omnipotent_format::Article =
            serde_json::from_slice(split(&response).1).unwrap();
        assert_eq!(decoded.title, title);
        assert_eq!(
            split(&server.request("GET", "/resources/0/metadata")).1,
            metadata
        );
        let response = server.request("GET", "/resources/256");
        assert!(
            split(&response)
                .0
                .to_ascii_lowercase()
                .contains("content-type: application/pdf")
        );
        assert_eq!(split(&response).1, pdf);
        let response = server.request("HEAD", "/resources/0");
        assert!(split(&response).1.is_empty());
        assert!(
            split(&response)
                .0
                .to_ascii_lowercase()
                .contains(&format!("content-length: {}", expected.len()))
        );
        assert!(
            split(&server.request("GET", "/resources/99999"))
                .0
                .starts_with("HTTP/1.1 404")
        );
    }
    // A cleanly closed but unfinished package is refused on startup.
    {
        let db = shodh_redb::Database::open(&path).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(META)
            .unwrap()
            .insert("progress", b"{\"ready\":false}".as_slice())
            .unwrap();
        txn.commit().unwrap();
    }
    assert!(omnipotent_linux::SourceMount::from_uri(&uri).is_err());
}

#[test]
fn embedded_opds2_sample_lists_and_downloads_an_epub_over_loopback() {
    let server = Server::sample();
    let response = server.request("GET", "/opds");
    let (head, body) = split(&response);
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/opds+json")
    );
    assert_eq!(body, omnipotent_serve::sample::CATALOG);
    let feed: serde_json::Value = serde_json::from_slice(body).unwrap();
    let publications = feed["publications"].as_array().unwrap();
    assert_eq!(publications.len(), 1);
    let link = &publications[0]["links"][0];
    assert_eq!(link["type"], "application/epub+zip");
    let download = server.request("GET", link["href"].as_str().unwrap());
    let (head, body) = split(&download);
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/epub+zip")
    );
    assert!(!head.to_ascii_lowercase().contains("content-encoding:"));
    assert_eq!(body, omnipotent_serve::sample::BOOK);
    let head = server.request("HEAD", "/books/connection-check.epub");
    assert!(split(&head).1.is_empty());
    let missing = server.request("GET", "/books/missing.epub");
    assert!(split(&missing).0.starts_with("HTTP/1.1 404"));
}

#[test]
fn sample_mode_does_not_silently_override_a_source() {
    let result = Command::new(env!("CARGO_BIN_EXE_omnipotent-linux"))
        .args(["--sample", "--source", "file:///not-opened"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("choose exactly one")
    );
}
