//! Immutable v1 bundle: 16-byte header, cluster row boundaries, centroids,
//! codebooks, codes, vectors, passage references, article byte boundaries, records.
//! The last cluster boundary gives the passage count; article boundaries are
//! relative to the start of the record arena. All scalar fields are little-endian.
//! The reader borrows every section; only search scratch is allocated.
use alloc::{collections::BinaryHeap, vec, vec::Vec};
use core::{cmp::Ordering, mem::size_of};
use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
    little_endian::{F32, U32},
};

use crate::{DocumentId, Record, RecordError, package::PassageRef};

const MAGIC: [u8; 8] = *b"OMNIBIN\x01";
pub const DIMENSIONS: usize = 256;
pub const SUBVECTORS: usize = 32;
pub const CODEWORDS: usize = 256;
pub const SUB_DIM: usize = DIMENSIONS / SUBVECTORS;
pub type Vector = [F32; DIMENSIONS];
pub type Code = [u8; SUBVECTORS];
pub type Codebook = [[F32; SUB_DIM]; CODEWORDS];

#[repr(C)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
pub struct Header {
    magic: [u8; 8],
    clusters: U32,
    articles: U32,
}

impl Header {
    pub fn new(clusters: u32, articles: u32) -> Self {
        Self {
            magic: MAGIC,
            clusters: U32::new(clusters),
            articles: U32::new(articles),
        }
    }
}

pub struct Bundle<'a> {
    clusters: &'a [U32],
    centroids: &'a [Vector],
    codebooks: &'a [Codebook; SUBVECTORS],
    codes: &'a [Code],
    vectors: &'a [Vector],
    passages: &'a [PassageRef],
    article_offsets: &'a [U32],
    articles: &'a [u8],
}

fn take<'a, T: FromBytes + KnownLayout + Immutable + Unaligned>(
    rest: &mut &'a [u8],
    count: usize,
) -> Result<&'a [T], RecordError> {
    let n = size_of::<T>().checked_mul(count).ok_or(RecordError::Size)?;
    let (bytes, tail) = rest.split_at_checked(n).ok_or(RecordError::Size)?;
    *rest = tail;
    <[T]>::ref_from_bytes(bytes).map_err(|_| RecordError::Size)
}

fn boundaries(offsets: &[U32], allow_empty: bool) -> Result<usize, RecordError> {
    if offsets[0].get() != 0
        || offsets
            .windows(2)
            .any(|w| w[0].get() > w[1].get() || (!allow_empty && w[0] == w[1]))
    {
        return Err(RecordError::Invalid);
    }
    Ok(offsets[offsets.len() - 1].get() as usize)
}

impl<'a> Bundle<'a> {
    pub fn parse(mut bytes: &'a [u8]) -> Result<Self, RecordError> {
        let header = &take::<Header>(&mut bytes, 1)?[0];
        if header.magic != MAGIC {
            return Err(RecordError::Invalid);
        }
        let nc = header.clusters.get() as usize;
        let na = header.articles.get() as usize;
        let clusters = take::<U32>(&mut bytes, nc.checked_add(1).ok_or(RecordError::Size)?)?;
        let rows = boundaries(clusters, true)?;
        let centroids = take(&mut bytes, nc)?;
        let codebooks = &take(&mut bytes, 1)?[0];
        let codes = take(&mut bytes, rows)?;
        let vectors = take(&mut bytes, rows)?;
        let passages = take(&mut bytes, rows)?;
        let article_offsets = take::<U32>(&mut bytes, na.checked_add(1).ok_or(RecordError::Size)?)?;
        if boundaries(article_offsets, false)? != bytes.len() {
            return Err(RecordError::Size);
        }
        Ok(Self {
            clusters,
            centroids,
            codebooks,
            codes,
            vectors,
            passages,
            article_offsets,
            articles: bytes,
        })
    }

    pub fn codes(&self) -> &'a [Code] {
        self.codes
    }
    pub fn vectors(&self) -> &'a [Vector] {
        self.vectors
    }
    pub fn passages(&self) -> &'a [PassageRef] {
        self.passages
    }
    pub fn article_count(&self) -> usize {
        self.article_offsets.len() - 1
    }

    pub fn article(&self, id: DocumentId) -> Result<Record<'a>, RecordError> {
        Record::parse(self.article_bytes(id)?)
    }

    pub fn article_bytes(&self, id: DocumentId) -> Result<&'a [u8], RecordError> {
        let mut offsets = self.article_offsets.iter().skip(id.0 as usize);
        let start = offsets.next().ok_or(RecordError::Invalid)?.get() as usize;
        let end = offsets.next().ok_or(RecordError::Invalid)?.get() as usize;
        Ok(&self.articles[start..end])
    }

    /// Query and stored vectors are unit-normalized 256d embeddings.
    /// PQ only picks candidates; all returned scores are exact dot products.
    pub fn search(
        &self,
        query: &[f32; DIMENSIONS],
        probes: usize,
        shortlist: usize,
        k: usize,
    ) -> Result<Vec<Hit>, RecordError> {
        let norm = query.iter().map(|v| v * v).sum::<f32>();
        if !(0.999..=1.001).contains(&norm) || k == 0 || shortlist < k || probes == 0 {
            return Err(RecordError::Invalid);
        }
        if self.codes.is_empty() {
            return Ok(Vec::new());
        }
        let mut clusters = BinaryHeap::new();
        let probes = probes.min(self.centroids.len());
        for (id, center) in self.centroids.iter().enumerate() {
            let projection = dot(query, center);
            let squared_norm = center.iter().map(|x| x.get() * x.get()).sum::<f32>();
            // Signed squared cosine preserves ordering without needing sqrt in no_std.
            let proximity = if squared_norm > 0. {
                projection * projection.abs() / squared_norm
            } else {
                0.
            };
            keep(
                &mut clusters,
                probes,
                Hit {
                    row: id as u32,
                    score: proximity,
                },
            );
        }
        let mut lut = vec![0.; SUBVECTORS * CODEWORDS];
        for (m, book) in self.codebooks.iter().enumerate() {
            let q = &query[m * SUB_DIM..(m + 1) * SUB_DIM];
            for (i, word) in book.iter().enumerate() {
                lut[m * CODEWORDS + i] = dot(q, word);
            }
        }
        let mut candidates = BinaryHeap::new();
        let shortlist = shortlist.min(self.codes.len());
        for cluster in clusters {
            let i = cluster.row as usize;
            let bias = dot(query, &self.centroids[i]);
            for row in self.clusters[i].get()..self.clusters[i + 1].get() {
                let approx = self.codes[row as usize]
                    .iter()
                    .enumerate()
                    .map(|(m, code)| lut[m * CODEWORDS + usize::from(*code)])
                    .sum::<f32>();
                keep(
                    &mut candidates,
                    shortlist,
                    Hit {
                        row,
                        score: bias + approx,
                    },
                );
            }
        }
        let mut hits: Vec<_> = candidates
            .into_iter()
            .map(|hit| Hit {
                row: hit.row,
                score: dot(query, &self.vectors[hit.row as usize]),
            })
            .collect();
        hits.sort_unstable();
        hits.truncate(k);
        Ok(hits)
    }
}

fn dot(a: &[f32], b: &[F32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y.get()).sum()
}

fn keep(heap: &mut BinaryHeap<Hit>, limit: usize, hit: Hit) {
    if heap.len() < limit {
        heap.push(hit);
    } else if let Some(mut worst) = heap.peek_mut()
        && hit < *worst
    {
        *worst = hit;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub row: u32,
    pub score: f32,
}
impl Ord for Hit {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then(self.row.cmp(&other.row))
    }
}
impl PartialOrd for Hit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Hit {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Hit {}
