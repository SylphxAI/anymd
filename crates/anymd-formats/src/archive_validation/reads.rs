//! Supported entry encodings and cumulative read accounting.

use zip::CompressionMethod;

use super::archive;
use crate::ooxml::{guard_zip, Package};
use crate::ConvertError;

#[test]
fn stored_deflated_descriptor_and_zip64_entries_remain_readable() {
    for streamed in [false, true] {
        for large in [false, true] {
            for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
                let bytes = archive(streamed, large, method);
                guard_zip(&bytes).unwrap();
                let mut package = Package::open(&bytes, "test").unwrap();
                let entries = package.preflight().unwrap();
                assert_eq!(entries.len(), 2);
                for (index, entry) in entries.iter().enumerate() {
                    assert_eq!(entry.index, index);
                    assert_eq!(entry.inflated, b"first payload".len());
                }
                assert_eq!(package.read("first").unwrap().unwrap(), b"first payload");
                assert_eq!(package.read("other").unwrap().unwrap(), b"other payload");
            }
        }
    }
}

#[test]
fn timestamps_are_not_entry_identity() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    // Some ordinary writers record different timestamps in the two headers.
    bytes[10..14].fill(0);
    guard_zip(&bytes).unwrap();
    let mut package = Package::open(&bytes, "test").unwrap();
    assert_eq!(package.read("first").unwrap().unwrap(), b"first payload");
}

#[test]
fn preflight_remeasures_each_index_and_keeps_the_cumulative_charge() {
    let bytes = archive(false, false, CompressionMethod::Deflated);
    let mut package = Package::open(&bytes, "test").unwrap();
    package.limits = crate::limits::Limits::with_inflated_limit(51);
    let entries = package.preflight().unwrap();
    assert_eq!(
        entries.iter().map(|entry| entry.inflated).sum::<usize>(),
        26
    );
    assert_eq!(
        package.preflight().unwrap_err(),
        ConvertError::limit("inflated input", 51)
    );
}
