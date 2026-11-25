//! Rejection of inconsistent entry headers and overlapping ranges.

use std::io::Cursor;

use zip::{CompressionMethod, ZipArchive};

use super::archive;
use crate::ooxml::guard_zip;
use crate::{ConvertError, Format, Options};

fn offsets(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|index| {
            let file = zip.by_index_raw(index).unwrap();
            (
                file.header_start() as usize,
                file.central_header_start() as usize,
                file.data_start().unwrap() as usize,
            )
        })
        .collect()
}

fn refused(bytes: &[u8]) {
    let error = guard_zip(bytes).expect_err("malformed container must be refused");
    assert!(matches!(error, ConvertError::Invalid(_)), "{error:?}");
    // Every direct ZIP-based format entrypoint validates before optional parts
    // or a third-party parser can swallow the malformed entry.
    for format in [Format::Docx, Format::Xlsx, Format::Pptx, Format::Epub] {
        assert_eq!(
            crate::convert(format, bytes, &Options::default()),
            Err(error.clone())
        );
    }
}

#[test]
fn shared_local_header_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    let entries = offsets(&bytes);
    let first = (entries[0].0 as u32).to_le_bytes();
    bytes[entries[1].1 + 42..entries[1].1 + 46].copy_from_slice(&first);
    refused(&bytes);
}

#[test]
fn overlapping_payload_ranges_are_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    let entries = offsets(&bytes);
    // Keep the first local and central metadata consistent while extending
    // its stored payload through the next entry's independently valid payload.
    let length = (entries[1].2 + b"other payload".len() - entries[0].2) as u32;
    for start in [entries[0].0 + 18, entries[0].1 + 20] {
        bytes[start..start + 4].copy_from_slice(&length.to_le_bytes());
        bytes[start + 4..start + 8].copy_from_slice(&length.to_le_bytes());
    }
    refused(&bytes);
}

#[test]
fn local_name_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[30] = b'x';
    refused(&bytes);
}

#[test]
fn local_method_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[8] = 8;
    refused(&bytes);
}

#[test]
fn local_flags_mismatch_is_refused() {
    for bit in 0..16 {
        let mut bytes = archive(false, false, CompressionMethod::Stored);
        let flags = (1u16 << bit).to_le_bytes();
        bytes[6..8].copy_from_slice(&flags);
        refused(&bytes);
    }
}

#[test]
fn local_crc_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[14] ^= 1;
    refused(&bytes);
}

#[test]
fn local_compressed_size_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[18] ^= 1;
    refused(&bytes);
}

#[test]
fn local_inflated_size_mismatch_is_refused() {
    let mut bytes = archive(false, false, CompressionMethod::Stored);
    bytes[22] ^= 1;
    refused(&bytes);
}

#[test]
fn descriptor_nonzero_crc_mismatch_is_refused() {
    let mut bytes = archive(true, false, CompressionMethod::Deflated);
    bytes[14..18].copy_from_slice(&1u32.to_le_bytes());
    refused(&bytes);
}

#[test]
fn local_zip64_size_mismatch_is_refused() {
    let mut bytes = archive(false, true, CompressionMethod::Stored);
    // The writer reserves ZIP64 sizes in the local extra field.
    let extra = 30 + "first".len();
    assert_eq!(&bytes[extra..extra + 2], &1u16.to_le_bytes());
    bytes[extra + 4] ^= 1;
    refused(&bytes);
}
