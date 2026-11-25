//! Behavioral checks for stricter archive entry validation.

mod layout;
mod office;
mod reads;

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn archive(streamed: bool, large: bool, method: CompressionMethod) -> Vec<u8> {
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(large);
    let parts = [("first", b"first payload"), ("other", b"other payload")];
    if streamed {
        let mut zip = ZipWriter::new_stream(Vec::new());
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    } else {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
}
