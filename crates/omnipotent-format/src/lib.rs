//! Portable article records and mixed-resource packages (`no_std + alloc`).
//!
//! [`model`] contains owned construction types. [`record`] is their one stored
//! representation: a self-contained buffer with a directly borrowed node array.
//! [`package`] defines resource identities and fixed-size passage locations.
//! [`bundle`] borrows article records and the search index from one flat file.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod bundle;
pub mod model;
pub mod package;
pub mod record;
mod validation;

pub use model::*;
pub use record::{Record, RecordError};
pub use validation::{ModelError, ModelLimits};
