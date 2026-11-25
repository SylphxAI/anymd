//! Bounded OOXML ZIP access, archive validation, and part relationships.

mod reader;
mod relationships;
mod validation;

use crate::ConvertError;

pub(crate) use reader::{EntryInflation, Package};
pub(crate) use relationships::{Rel, Rels};
pub(crate) use validation::{check_limits, guard_zip, validate_archive};

#[cfg_attr(
    not(test),
    allow(unused_imports, reason = "retain the original package import")
)]
pub(super) use relationships::resolve_target;

/// Largest uncompressed zip entry we will inflate.
pub(crate) const MAX_ENTRY_BYTES: u64 = 200 * 1024 * 1024;
/// Largest number of entries a container may declare.
pub(crate) const MAX_ENTRIES: usize = 10_000;

pub(crate) fn invalid(message: impl std::fmt::Display) -> ConvertError {
    ConvertError::Invalid(message.to_string())
}
