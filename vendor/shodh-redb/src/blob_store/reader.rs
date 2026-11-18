use crate::{ReadTransaction, Result, StorageError, WriteTransaction};
use alloc::{vec, vec::Vec};

use super::BLOB_CHUNK_SIZE;

enum Source<'txn> {
    Read(&'txn ReadTransaction),
    Write(&'txn WriteTransaction),
}

/// A lazy, transaction-bound reader for blob data.
///
/// Opening the reader fetches metadata, not the blob body. [`Self::read_into`]
/// copies only the requested bytes into caller-owned memory. Database page/cache
/// allocations still apply; this is not kernel zero-copy.
///
/// Reads do not verify the whole-blob checksum. Use `get_blob` for a verified
/// whole-object read, or verify a digest while consuming the entire stream.
/// A read transaction pins the source snapshot for the reader's lifetime.
pub struct BlobReader<'txn> {
    source: Source<'txn>,
    source_sequence: u64,
    length: u64,
    position: u64,
}

impl<'txn> BlobReader<'txn> {
    pub(crate) fn from_read(txn: &'txn ReadTransaction, source_sequence: u64, length: u64) -> Self {
        Self {
            source: Source::Read(txn),
            source_sequence,
            length,
            position: 0,
        }
    }

    pub(crate) fn from_write(
        txn: &'txn WriteTransaction,
        source_sequence: u64,
        length: u64,
    ) -> Self {
        Self {
            source: Source::Write(txn),
            source_sequence,
            length,
            position: 0,
        }
    }

    /// Total length captured when this reader was opened.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.length
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.length.saturating_sub(self.position)
    }

    /// Set an absolute cursor, including past EOF. Available without `std`.
    pub fn seek_to(&mut self, position: u64) {
        self.position = position;
    }

    /// Read up to the buffer length, returning a short read at EOF.
    ///
    /// No output-sized temporary buffer is allocated. A storage failure leaves
    /// the cursor unchanged, but may have partially modified the output buffer.
    pub fn read_into(&mut self, output: &mut [u8]) -> Result<usize> {
        let count = usize::try_from(self.remaining().min(output.len() as u64))
            .map_err(|_| StorageError::Corrupted("blob read length exceeds usize".into()))?;
        if count == 0 {
            return Ok(0);
        }
        let output = &mut output[..count];
        match &self.source {
            Source::Read(txn) => {
                txn.read_blob_chunks_into(self.source_sequence, self.position, output)?;
            }
            Source::Write(txn) => {
                txn.read_blob_chunks_into(self.source_sequence, self.position, output)?;
            }
        }
        self.position += count as u64;
        Ok(count)
    }

    /// Allocate and read an exact range. Prefer `read_into` for bounded serving.
    /// On failure the cursor remains unchanged; zero-length reads are no-ops.
    pub fn read_range(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        if length == 0 {
            return Ok(Vec::new());
        }
        let end = offset
            .checked_add(length as u64)
            .filter(|end| *end <= self.length)
            .ok_or(StorageError::BlobRangeOutOfBounds {
                blob_length: self.length,
                requested_offset: offset,
                requested_length: length as u64,
            })?;
        let saved = self.position;
        self.position = offset;
        let mut output = vec![0; length];
        if let Err(error) = self.read_into(&mut output) {
            self.position = saved;
            return Err(error);
        }
        self.position = end;
        Ok(output)
    }
}

/// Split a previously bounds-checked request at physical chunk boundaries.
/// Each callback must copy exactly the requested chunk slice or fail.
pub(crate) fn copy_chunks(
    offset: u64,
    output: &mut [u8],
    mut copy: impl FnMut(u32, usize, &mut [u8]) -> Result,
) -> Result {
    let mut position = offset;
    let mut remaining = output;
    while !remaining.is_empty() {
        let index = u32::try_from(position / BLOB_CHUNK_SIZE as u64)
            .map_err(|_| StorageError::Corrupted("blob chunk index exceeds u32".into()))?;
        let local = usize::try_from(position % BLOB_CHUNK_SIZE as u64)
            .map_err(|_| StorageError::Corrupted("blob chunk offset exceeds usize".into()))?;
        let count = remaining.len().min(BLOB_CHUNK_SIZE - local);
        let (current, rest) = remaining.split_at_mut(count);
        copy(index, local, current)?;
        position = position
            .checked_add(count as u64)
            .ok_or_else(|| StorageError::Corrupted("blob read offset overflow".into()))?;
        remaining = rest;
    }
    Ok(())
}

pub(crate) fn copy_chunk(data: &[u8], offset: usize, output: &mut [u8]) -> Result {
    let end = offset
        .checked_add(output.len())
        .ok_or_else(|| StorageError::Corrupted("blob chunk range overflow".into()))?;
    let slice = data
        .get(offset..end)
        .ok_or_else(|| StorageError::Corrupted("missing or truncated blob chunk".into()))?;
    output.copy_from_slice(slice);
    Ok(())
}

#[cfg(feature = "std")]
impl std::io::Read for BlobReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.read_into(output).map_err(std::io::Error::other)
    }
}

#[cfg(feature = "std")]
impl std::io::Seek for BlobReader<'_> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            std::io::SeekFrom::Start(offset) => i64::try_from(offset).ok(),
            std::io::SeekFrom::End(offset) => i64::try_from(self.length)
                .ok()
                .and_then(|v| v.checked_add(offset)),
            std::io::SeekFrom::Current(offset) => i64::try_from(self.position)
                .ok()
                .and_then(|v| v.checked_add(offset)),
        };
        match new_pos {
            Some(pos) if pos >= 0 => {
                self.position = pos.unsigned_abs();
                Ok(self.position)
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }
}

impl core::fmt::Debug for BlobReader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlobReader")
            .field("source_sequence", &self.source_sequence)
            .field("blob_length", &self.length)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_requests_preserve_physical_offsets() {
        let mut calls = Vec::new();
        let mut output = [0; 9];
        copy_chunks(
            BLOB_CHUNK_SIZE as u64 - 2,
            &mut output,
            |index, offset, part| {
                calls.push((index, offset, part.len()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(calls, [(0, BLOB_CHUNK_SIZE - 2, 2), (1, 0, 7)]);
    }

    #[test]
    fn truncated_and_overflowing_chunk_slices_fail() {
        assert!(copy_chunk(&[1, 2], 1, &mut [0; 2]).is_err());
        assert!(copy_chunk(&[], usize::MAX, &mut [0; 1]).is_err());
    }

    #[test]
    fn chunk_index_overflow_is_rejected_before_reading() {
        let mut called = false;
        assert!(
            copy_chunks(
                (u64::from(u32::MAX) + 1) * BLOB_CHUNK_SIZE as u64,
                &mut [0; 1],
                |_, _, _| {
                    called = true;
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!called);
    }
}
