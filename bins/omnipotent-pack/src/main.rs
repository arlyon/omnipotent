use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use omnipotent_embed::{
    Chunker, DOCUMENT_TEMPLATE, Encoder, MODEL, MODEL_REVISION, QUERY_TEMPLATE,
};
use omnipotent_format::package::{EmbeddingProfile, ResourceKind};
use omnipotent_pack::{Package, merge};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Instant,
};

const DATASET_REVISION: &str = "417c267bb457fa645c22eb3b5c77764963194c70";

#[derive(Parser)]
#[command(
    version,
    about = "Prepare, embed, merge and finalise offline knowledge packages"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Ingect wikipedia from its structured export
    Wikipedia {
        #[arg(long)]
        input: Vec<PathBuf>,
        #[arg(long)]
        shards: Option<String>,
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long, default_value = DATASET_REVISION)]
        dataset_revision: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Resume embedding a prepared worker; already committed vectors are retained.
    Embed {
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        model: ModelArgs,
    },
    /// Take two or more fjall databases and merge them for finalization
    Merge {
        #[arg(long, required = true)]
        input: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Export a complete worker as an immutable .bin bundle.
    Finalize {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}
#[derive(clap::Args)]
struct ModelArgs {
    /// Directory containing original config.json, tokenizer.json and model.safetensors.
    #[arg(long)]
    model_dir: Option<PathBuf>,
    #[arg(long)]
    cache: Option<PathBuf>,
    #[arg(long, default_value = "auto", value_parser = ["auto", "cpu", "cuda"])]
    device: String,
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u16).range(1..=128))]
    batch_size: u16,
}
impl ModelArgs {
    fn cache(&self) -> PathBuf {
        self.cache
            .clone()
            .or_else(|| std::env::var_os("OMNIPOTENT_CACHE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(".cache/omnipotent-pack"))
    }
    fn directory(&self) -> Result<PathBuf> {
        if let Some(path) = &self.model_dir {
            return Ok(path.clone());
        }
        let directory = self
            .cache()
            .join(format!("embeddinggemma-2-{MODEL_REVISION}"));
        for name in ["config.json", "tokenizer.json", "model.safetensors"] {
            fetch(
                &format!("https://huggingface.co/{MODEL}/resolve/{MODEL_REVISION}/{name}"),
                &directory.join(name),
            )?;
        }
        Ok(directory)
    }
    fn embed(&self, package: &mut Package, directory: &Path) -> Result<()> {
        ensure!(
            package.profile().model == format!("{MODEL}@{MODEL_REVISION}"),
            "package was prepared for a different embedding model"
        );
        let start = Instant::now();
        let encoder = Encoder::load(directory, &self.device)?;
        eprintln!("model loading: {:.3}s", start.elapsed().as_secs_f64());
        let start = Instant::now();
        package.embed(&encoder, self.batch_size as usize)?;
        eprintln!("inference + commits: {:.3}s", start.elapsed().as_secs_f64());
        Ok(())
    }
}
fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Wikipedia {
            mut input,
            shards,
            language,
            dataset_revision,
            output,
            limit,
            model,
        } => {
            ensure!(
                !output.exists(),
                "output already exists: {}",
                output.display()
            );
            ensure!(limit != Some(0), "limit must be positive");
            ensure!(
                shards.is_none() || input.is_empty(),
                "choose --input or --shards, not both"
            );
            if let Some(range) = shards {
                input = wikipedia_shards(&range, &language, &dataset_revision, &model.cache())?;
            }
            ensure!(
                !input.is_empty(),
                "provide local --input Parquet shards or --shards START..END"
            );
            let directory = model.directory()?;
            let chunker = Chunker::load(&directory.join("tokenizer.json"))?;
            let profile = EmbeddingProfile {
                model: format!("{MODEL}@{MODEL_REVISION}"),
                files: BTreeMap::new(),
                dimensions: 768,
                document_template: DOCUMENT_TEMPLATE.into(),
                query_template: QUERY_TEMPLATE.into(),
            };
            let mut package = Package::create(&output, &profile)?;
            let start = Instant::now();
            let mut articles = 0;
            'shards: for path in input {
                for article in omnipotent_wikipedia::articles(File::open(&path)?)? {
                    let article = article.with_context(|| format!("source {}", path.display()))?;
                    let passages =
                        chunker.chunk(ResourceKind::Article, &article, package.next_document())?;
                    package.add_article(&article, &passages)?;
                    articles += 1;
                    if limit.is_some_and(|n| articles >= n) {
                        break 'shards;
                    }
                }
            }
            package.finish_preparation()?;
            eprintln!(
                "prepared {articles} articles in {:.3}s",
                start.elapsed().as_secs_f64()
            );
            model.embed(&mut package, &directory)?;
        }
        Command::Embed { output, model } => {
            let mut package = Package::open(&output)?;
            let directory = model.directory()?;
            model.embed(&mut package, &directory)?;
        }
        Command::Merge { input, output } => {
            merge(&input, &output)?;
        }
        Command::Finalize { input, output } => Package::open(&input)?.finalize(&output)?,
    }
    Ok(())
}
fn fetch(url: &str, path: &Path) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("cache directory")?)?;
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    eprintln!("downloading {}", path.display());
    let mut response = ureq::get(url).call()?;
    let mut file = File::create(&partial)?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, path)?;
    Ok(())
}
fn wikipedia_shards(
    range: &str,
    language: &str,
    revision: &str,
    cache: &Path,
) -> Result<Vec<PathBuf>> {
    ensure!(
        ["en", "fr"].contains(&language),
        "dataset supports en or fr"
    );
    let (start, end) = range
        .split_once("..")
        .context("shards must be START..END, end-exclusive")?;
    let start: usize = start.parse()?;
    let end: usize = end.parse()?;
    #[derive(Deserialize)]
    struct Entry {
        path: String,
    }
    let url = format!(
        "https://huggingface.co/api/datasets/wikimedia/structured-wikipedia/tree/{revision}/{language}wiki/data?limit=1000"
    );
    let mut files: Vec<Entry> = ureq::get(&url).call()?.body_mut().read_json()?;
    files.retain(|f| f.path.ends_with(".parquet"));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    ensure!(
        start < end && end <= files.len(),
        "invalid shard range; available shards: {}",
        files.len()
    );
    files[start..end].iter().map(|entry| {
        let name = entry.path.rsplit('/').next().context("source shard filename")?;
        let path = cache.join(format!("wikipedia-{language}-{revision}")).join(name);
        fetch(&format!("https://huggingface.co/datasets/wikimedia/structured-wikipedia/resolve/{revision}/{}", entry.path), &path)?;
        Ok(path)
    }).collect()
}
