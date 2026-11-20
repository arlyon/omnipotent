use omnipotent_linux::SourceMount;
use std::{
    env,
    error::Error,
    io::{self, Write},
    net::SocketAddr,
    time::Duration,
};
use tokio::{
    net::TcpListener,
    task::{JoinSet, LocalSet},
};

const HELP: &str = "omnipotent-linux (--sample | --source file:///absolute/source) [--listen 127.0.0.1:8080]\n\n--sample serves the flash-backed OPDS 2.0 test catalog at /opds and its EPUB.\nRead-only native-file directory or finalized .bin bundle. GET/HEAD.\nDirectory: / resolves index.html; confined paths require Linux 5.6+. Relative\nsymlinks within the mount are allowed; symlink escapes are rejected.\nBundle: /resources/<bundle-local-u32> returns an article record; append\n/metadata for shared Resource JSON.\nSources must publish by atomic replacement, never modify an in-flight file.\n16 concurrent connections, 30-second total deadline each. Ctrl-C stops.\nNo client compression requirement, rendering, search, auth, or remote sync.\n";

fn main() -> Result<(), Box<dyn Error>> {
    let mut source = None;
    let mut sample = false;
    let mut listen: SocketAddr = "127.0.0.1:8080".parse()?;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--sample" if !sample => sample = true,
            "--source" if source.is_none() => {
                source = Some(args.next().ok_or("--source requires a URI")?)
            }
            "--listen" => listen = args.next().ok_or("--listen requires an address")?.parse()?,
            _ => return Err(format!("unknown or repeated argument: {arg}\n{HELP}").into()),
        }
    }
    let (source, mount) = match (source, sample) {
        (Some(uri), false) => {
            let mount = SourceMount::from_uri(&uri)?;
            (uri, mount)
        }
        (None, true) => ("embedded-sample".into(), SourceMount::sample()),
        _ => return Err(format!("choose exactly one of --source or --sample\n{HELP}").into()),
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(LocalSet::new().run_until(async move {
            let listener = TcpListener::bind(listen).await?;
            println!(
                "LISTENING http://{} source={source}",
                listener.local_addr()?
            );
            io::stdout().flush()?;
            let mut tasks = JoinSet::new();
            let stop = tokio::signal::ctrl_c();
            tokio::pin!(stop);
            loop {
                tokio::select! {
                    result = &mut stop => {
                        result?;
                        tasks.shutdown().await;
                        return Ok::<(), io::Error>(());
                    }
                    accepted = listener.accept(), if tasks.len() < 16 => {
                        let (socket, _) = accepted?;
                        let mount = mount.clone();
                        tasks.spawn_local(async move {
                            mount.serve_socket(socket, Duration::from_secs(30)).await
                        });
                    }
                    result = tasks.join_next(), if !tasks.is_empty() => {
                        match result {
                            Some(Ok(Err(error))) => eprintln!("connection: {error}"),
                            Some(Err(error)) => eprintln!("connection task: {error}"),
                            _ => {}
                        }
                    }
                }
            }
        }))?;
    Ok(())
}
