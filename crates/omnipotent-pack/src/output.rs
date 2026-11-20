use super::{Package, RawVector};
use anyhow::{Context, Result, ensure};
use ndarray::{Array2, Array3, ArrayView1, Axis};
use omnipotent_format::{
    bundle::{CODEWORDS, Code, DIMENSIONS, Header, SUB_DIM, SUBVECTORS},
    package::PassageRef,
};
use rand::{SeedableRng, rngs::StdRng};
use reductive::{
    kmeans::{KMeans, NIterationsCondition, RandomInstanceCentroids},
    pq::{Pq, QuantizeVector, TrainPq},
};
use std::{
    io::{BufWriter, Write},
    path::Path,
};
use zerocopy::{
    FromBytes, IntoBytes,
    little_endian::{F32, U32},
};

const SAMPLE: u64 = 8192;

fn reduce(bytes: &[u8]) -> Result<[f32; DIMENSIONS]> {
    let raw =
        RawVector::ref_from_bytes(bytes).map_err(|_| anyhow::anyhow!("invalid worker vector"))?;
    let values: [f32; DIMENSIONS] = core::array::from_fn(|i| raw[i].get());
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    ensure!(norm.is_finite() && norm > 0., "invalid 256d embedding norm");
    Ok(values.map(|v| v / norm))
}

fn nearest(centers: &Array2<f32>, row: ArrayView1<'_, f32>) -> usize {
    centers
        .outer_iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let score = |c: &ArrayView1<'_, f32>| {
                let norm = c.dot(c).sqrt();
                if norm > 0. { row.dot(c) / norm } else { 0. }
            };
            score(a).total_cmp(&score(b))
        })
        .map_or(0, |(cluster, _)| cluster)
}

fn train(worker: &Package, clusters: usize) -> Result<(Array2<f32>, Pq<f32>)> {
    let n = worker.progress.passages;
    let samples = n.min(SAMPLE);
    let mut data = Vec::with_capacity(samples as usize * DIMENSIONS);
    for i in 0..samples {
        let id = i * n / samples;
        let vector = worker
            .vectors
            .get(id.to_be_bytes())?
            .context("missing training vector")?;
        data.extend(reduce(&vector)?);
    }
    let mut residuals = Array2::from_shape_vec((samples as usize, DIMENSIONS), data)?;
    let mut rng = StdRng::seed_from_u64(42);
    let centers = if clusters == 0 {
        Array2::zeros((0, DIMENSIONS))
    } else if clusters == 1 {
        residuals
            .mean_axis(Axis(0))
            .context("empty training set")?
            .insert_axis(Axis(0))
    } else {
        residuals
            .k_means(
                Axis(0),
                clusters,
                RandomInstanceCentroids::new(&mut rng),
                NIterationsCondition(10),
            )
            .0
    };
    for mut row in residuals.outer_iter_mut() {
        let cluster = nearest(&centers, row.view());
        row -= &centers.row(cluster);
    }
    let pq = if samples < CODEWORDS as u64 {
        // With fewer than 256 points, each residual subvector can have an exact codeword.
        Pq::new(
            None,
            Array3::from_shape_fn((SUBVECTORS, CODEWORDS, SUB_DIM), |(m, c, d)| {
                if samples == 0 {
                    0.
                } else {
                    residuals[[c % samples as usize, m * SUB_DIM + d]]
                }
            }),
        )
    } else {
        Pq::train_pq_using(SUBVECTORS, 8, 10, 1, residuals.view(), &mut rng)?
    };
    ensure!(
        centers
            .iter()
            .chain(pq.subquantizers().iter())
            .all(|v| v.is_finite()),
        "non-finite index training result"
    );
    Ok((centers, pq))
}

pub(super) fn write(worker: &Package, output: &Path) -> Result<()> {
    ensure!(
        !output.exists(),
        "output already exists: {}",
        output.display()
    );
    let n = u32::try_from(worker.progress.passages)?;
    let docs = worker.progress.documents;
    let clusters = if n == 0 {
        0
    } else {
        ((f64::from(n).sqrt() as usize) / 8).clamp(1, 256)
    };
    let (centers, pq) = train(worker, clusters)?;
    let mut rows = vec![Vec::<(u32, Code)>::new(); clusters];
    for id in 0..n {
        let vector = worker
            .vectors
            .get(u64::from(id).to_be_bytes())?
            .context("missing vector")?;
        let values = reduce(&vector)?;
        let row = ArrayView1::from(&values);
        let cluster = nearest(&centers, row);
        let code = pq.quantize_vector::<u8, _>(row.to_owned() - centers.row(cluster));
        let code: Code = code
            .as_slice()
            .and_then(|s| s.try_into().ok())
            .context("invalid trained PQ code")?;
        rows[cluster].push((id, code));
    }

    let dir = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    {
        let mut out = BufWriter::new(file.as_file_mut());
        out.write_all(Header::new(clusters as u32, docs).as_bytes())?;
        let mut offset = 0;
        out.write_all(U32::new(offset).as_bytes())?;
        for cluster in &rows {
            offset += u32::try_from(cluster.len())?;
            out.write_all(U32::new(offset).as_bytes())?;
        }
        for &f in centers.iter().chain(pq.subquantizers().iter()) {
            out.write_all(F32::new(f).as_bytes())?;
        }
        for (_, code) in rows.iter().flatten() {
            out.write_all(code)?;
        }
        for &(id, _) in rows.iter().flatten() {
            let vector = worker
                .vectors
                .get(u64::from(id).to_be_bytes())?
                .context("missing vector")?;
            out.write_all(reduce(&vector)?.map(F32::new).as_bytes())?;
        }
        for &(id, _) in rows.iter().flatten() {
            let reference = worker
                .passages
                .get(u64::from(id).to_be_bytes())?
                .context("missing passage")?;
            let reference = PassageRef::parse(&reference)?;
            ensure!(reference.resource().0 < docs, "unknown passage document");
            out.write_all(reference.bytes())?;
        }
        let mut offset = 0u32;
        out.write_all(U32::new(offset).as_bytes())?;
        for id in 0..docs {
            let article = worker
                .articles
                .get(id.to_be_bytes())?
                .context("missing article")?;
            offset = offset
                .checked_add(u32::try_from(article.len())?)
                .context("article arena exceeds 4 GiB")?;
            out.write_all(U32::new(offset).as_bytes())?;
        }
        for id in 0..docs {
            let article = worker
                .articles
                .get(id.to_be_bytes())?
                .context("missing article")?;
            out.write_all(&article)?;
        }
        out.flush()?;
    }
    file.as_file().sync_all()?;
    file.persist_noclobber(output).map_err(|e| e.error)?;
    Ok(())
}
