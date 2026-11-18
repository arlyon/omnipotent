//! Portable mixed-resource packages and structured Articles (`no_std + alloc`).
//!
//! The logical model uses `alloc` and Serde. [`ResourceId`] names resources across
//! logical sources, independently of titles, mirror URLs, and runtime document
//! IDs. Call [`Article::validate`] after deserializing it.
//! [`package`] defines the common resource catalogue and search targets.
//! With `storage`, [`reader`] reads the same tables written by the host packer.
//! [`Record`] is the legacy compressed envelope, not the current package payload.
//!
//! The legacy envelope parser can locate stored bytes without constructing an article:
//!
//! ```
//! use omnipotent_format::{DocumentId, RecordHeader, RecordLimits};
//!
//! let header = RecordHeader::new(DocumentId(7), 128, 512)?;
//! let borrowed = RecordHeader::parse(header.bytes(), RecordLimits::default())?;
//! assert_eq!(borrowed.document(), DocumentId(7));
//! let offset = 1_u64 << 32;
//! assert_eq!(borrowed.payload_range(offset)?, offset + 40..offset + 168);
//! # Ok::<(), omnipotent_format::RecordError>(())
//! ```

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod model;
pub mod package;
#[cfg(feature = "storage")]
pub mod reader;
pub mod record;
mod validation;

pub use model::*;
pub use record::{RECORD_HEADER_LEN, Record, RecordError, RecordHeader, RecordLimits};
pub use validation::{ModelError, ModelLimits};
