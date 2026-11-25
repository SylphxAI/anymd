use std::path::Path;

/// File extensions a directory walk picks up for search.
pub fn searchable_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "pdf"
            | "docx"
            | "pptx"
            | "xlsx"
            | "xls"
            | "xlsm"
            | "ods"
            | "csv"
            | "tsv"
            | "epub"
            | "html"
            | "htm"
            | "xhtml"
            | "md"
            | "markdown"
            | "txt"
            | "rst"
            | "srt"
            | "vtt"
    )
}

/// Readable file extensions (search set plus media).
pub fn readable_extension(path: &Path) -> bool {
    searchable_extension(path)
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "tif"
                        | "tiff"
                        | "bmp"
                        | "mp4"
                        | "mov"
                        | "mkv"
                        | "webm"
                        | "mp3"
                        | "wav"
                        | "m4a"
                        | "flac"
                        | "ogg"
                )
            })
}
