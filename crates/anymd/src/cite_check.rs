//! Bounded inspect cite-check orchestration over the existing text/OCR owners.

mod evaluation;
mod evidence;
mod request;
mod response;
mod source;
mod worker;

pub use request::{normalize_args, normalize_citation};
pub(crate) use worker::inspect;
pub use worker::worker;

#[cfg(test)]
mod tests;
