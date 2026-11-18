//! Legacy v2 framing around an opaque Zstd-compressed article JSON payload.
//! Current mixed-resource packages do not use this envelope.
//!
//! Parsing checks framing and the Zstd magic, not decompression, checksums,
//! actual decoded size, or article validity. Those belong to the consumer.

use core::{
    fmt,
    mem::{align_of, size_of},
    ops::Range,
};
use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::DocumentId;

pub const RECORD_HEADER_LEN: usize = 40;
const MAGIC: [u8; 8] = *b"OMNIREF\0";
const VERSION: u16 = 2;
const ARTICLE_JSON: u16 = 1;
const ZSTD: u16 = 1;
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Header fields have byte alignment and explicit little-endian representation.
/// Use `parse`, not a raw zerocopy cast, to validate the format invariants.
#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct RecordHeader {
    magic: [u8; 8],
    version: U16,
    header_len: U16,
    flags: U32,
    document: U32,
    payload_format: U16,
    compression: U16,
    encoded_len: U64,
    decoded_len: U64,
}

const _: [(); RECORD_HEADER_LEN] = [(); size_of::<RecordHeader>()];
const _: [(); 1] = [(); align_of::<RecordHeader>()];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordLimits {
    pub max_encoded_len: u64,
    pub max_decoded_len: u64,
}

impl Default for RecordLimits {
    fn default() -> Self {
        Self {
            max_encoded_len: 17 * 1024 * 1024,
            max_decoded_len: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    HeaderSize,
    Magic,
    Version(u16),
    HeaderLength(u16),
    Flags(u32),
    PayloadFormat(u16),
    Compression(u16),
    EmptyPayload,
    EncodedLimit,
    DecodedLimit,
    RangeOverflow,
    Truncated,
    TrailingBytes,
    ZstdMagic,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for RecordError {}

impl RecordHeader {
    pub fn new(
        document: DocumentId,
        encoded_len: u64,
        decoded_len: u64,
    ) -> Result<Self, RecordError> {
        if encoded_len == 0 || decoded_len == 0 {
            return Err(RecordError::EmptyPayload);
        }
        encoded_len
            .checked_add(RECORD_HEADER_LEN as u64)
            .ok_or(RecordError::RangeOverflow)?;
        Ok(Self {
            magic: MAGIC,
            version: U16::new(VERSION),
            header_len: U16::new(RECORD_HEADER_LEN as u16),
            flags: U32::new(0),
            document: U32::new(document.0),
            payload_format: U16::new(ARTICLE_JSON),
            compression: U16::new(ZSTD),
            encoded_len: U64::new(encoded_len),
            decoded_len: U64::new(decoded_len),
        })
    }

    /// Borrow exactly one header, without requiring native alignment.
    pub fn parse(bytes: &[u8], limits: RecordLimits) -> Result<&Self, RecordError> {
        let header = Self::ref_from_bytes(bytes).map_err(|_| RecordError::HeaderSize)?;
        header.validate(limits)?;
        Ok(header)
    }

    pub fn validate(&self, limits: RecordLimits) -> Result<(), RecordError> {
        if self.magic != MAGIC {
            return Err(RecordError::Magic);
        }
        if self.version.get() != VERSION {
            return Err(RecordError::Version(self.version.get()));
        }
        if self.header_len.get() != RECORD_HEADER_LEN as u16 {
            return Err(RecordError::HeaderLength(self.header_len.get()));
        }
        if self.flags.get() != 0 {
            return Err(RecordError::Flags(self.flags.get()));
        }
        if self.payload_format.get() != ARTICLE_JSON {
            return Err(RecordError::PayloadFormat(self.payload_format.get()));
        }
        if self.compression.get() != ZSTD {
            return Err(RecordError::Compression(self.compression.get()));
        }
        if self.encoded_len() == 0 || self.decoded_len() == 0 {
            return Err(RecordError::EmptyPayload);
        }
        if self.encoded_len() > limits.max_encoded_len {
            return Err(RecordError::EncodedLimit);
        }
        if self.decoded_len() > limits.max_decoded_len {
            return Err(RecordError::DecodedLimit);
        }
        self.payload_range(0)?;
        Ok(())
    }

    pub fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }
    pub fn document(&self) -> DocumentId {
        DocumentId(self.document.get())
    }
    pub fn encoded_len(&self) -> u64 {
        self.encoded_len.get()
    }
    pub fn decoded_len(&self) -> u64 {
        self.decoded_len.get()
    }

    /// Locate the encoded body in a larger object. No truncating `usize` casts.
    pub fn payload_range(&self, record_offset: u64) -> Result<Range<u64>, RecordError> {
        let start = record_offset
            .checked_add(RECORD_HEADER_LEN as u64)
            .ok_or(RecordError::RangeOverflow)?;
        let end = start
            .checked_add(self.encoded_len())
            .ok_or(RecordError::RangeOverflow)?;
        Ok(start..end)
    }
}

/// A borrowed record; construction never parses or copies the article body.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    header: &'a RecordHeader,
    payload: &'a [u8],
}

impl<'a> Record<'a> {
    pub fn parse(bytes: &'a [u8], limits: RecordLimits) -> Result<Self, RecordError> {
        let (record, remaining) = Self::parse_prefix(bytes, limits)?;
        if !remaining.is_empty() {
            return Err(RecordError::TrailingBytes);
        }
        Ok(record)
    }

    /// Also permits sequential scans over concatenated framed records.
    pub fn parse_prefix(
        bytes: &'a [u8],
        limits: RecordLimits,
    ) -> Result<(Self, &'a [u8]), RecordError> {
        let header_bytes = bytes
            .get(..RECORD_HEADER_LEN)
            .ok_or(RecordError::Truncated)?;
        let header = RecordHeader::parse(header_bytes, limits)?;
        let end = usize::try_from(header.payload_range(0)?.end)
            .map_err(|_| RecordError::RangeOverflow)?;
        let payload = bytes
            .get(RECORD_HEADER_LEN..end)
            .ok_or(RecordError::Truncated)?;
        if !payload.starts_with(&ZSTD_MAGIC) {
            return Err(RecordError::ZstdMagic);
        }
        Ok((Self { header, payload }, &bytes[end..]))
    }

    pub fn header(&self) -> &'a RecordHeader {
        self.header
    }
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
}
