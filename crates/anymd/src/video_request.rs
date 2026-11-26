//! Public video integration: admission precedes cache/worker work. Reuses the
//! existing supervised self-worker, OCR permit, local caption adapter and image store.

mod cache;
mod client;
mod inspect;
mod sidecars;
mod worker;

pub use client::TimelineSelection;
pub(crate) use client::{document, manifest_id};
pub(crate) use inspect::inspect;
pub use worker::worker;

#[cfg(test)]
mod tests;
