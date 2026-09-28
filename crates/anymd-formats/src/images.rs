//! Images embedded in documents (PDF figures, DOCX/PPTX/EPUB pictures): a
//! content-addressed export cache and the Markdown that points into it.
//!
//! Each raster image is written once to `<cache>/images/<first 16 hex of its
//! SHA-256>.<ext>`, so the same picture is one file however often it is read,
//! and nothing is ever written beside the source document. The Markdown holds
//! the absolute path, which an agent can open natively.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Images with more pixels than this are refused (checked on the header, before decoding).
pub const MAX_PIXELS: u64 = 50_000_000;
/// Images narrower or shorter than this are decoration.
pub const MIN_SIDE_PX: u32 = 48;
/// The same picture on this many pages, slides or chapters is a logo or an ornament.
pub const REPEAT_UNITS: usize = 3;

/// An exported image file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredImage {
    /// Absolute path of the file in the cache.
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// Where exported images go.
#[derive(Debug, Clone)]
pub struct ImageStore {
    dir: PathBuf,
}

impl ImageStore {
    /// A store writing into `dir` (made absolute).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        Self { dir }
    }

    /// `<anymd cache>/images`; `None` when no cache directory can be derived.
    pub fn default_location() -> Option<Self> {
        crate::cache::cache_dir().map(|root| Self::at(root.join("images")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Write `bytes` to the cache (once) and return the file. Fails for
    /// anything that is not a raster image, and for images over 50 megapixels.
    #[cfg(feature = "native")]
    pub fn put(&self, bytes: &[u8]) -> Result<StoredImage, String> {
        use sha2::{Digest, Sha256};
        use std::io::Write;

        let (width, height, ext) =
            inspect(bytes).ok_or("not a raster image anymd exports, or over 50 megapixels")?;
        let hash: String = Sha256::digest(bytes)
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = self.dir.join(format!("{hash}.{ext}"));
        if !path.is_file() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|error| format!("cannot create {}: {error}", self.dir.display()))?;
            let mut file = tempfile::NamedTempFile::new_in(&self.dir)
                .map_err(|error| format!("cannot write in {}: {error}", self.dir.display()))?;
            file.write_all(bytes)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            // Another reader may have written the same file meanwhile.
            if let Err(error) = file.persist(&path) {
                if !path.is_file() {
                    return Err(format!("cannot write {}: {error}", path.display()));
                }
            }
        }
        Ok(StoredImage {
            path,
            width,
            height,
        })
    }

    #[cfg(not(feature = "native"))]
    pub fn put(&self, _bytes: &[u8]) -> Result<StoredImage, String> {
        Err("exporting images is not available in this build".into())
    }
}

/// Width, height and file extension of a raster image (PNG, JPEG, GIF, WebP,
/// TIFF, BMP), read from its header. `None` for anything else, and for images
/// over [`MAX_PIXELS`].
pub fn inspect(bytes: &[u8]) -> Option<(u32, u32, &'static str)> {
    let ext = match crate::image::kind(bytes)? {
        "PNG" => "png",
        "JPEG" => "jpg",
        "GIF" => "gif",
        "WebP" => "webp",
        "TIFF" => "tiff",
        "BMP" => "bmp",
        _ => return None,
    };
    let size = imagesize::blob_size(bytes).ok()?;
    let width = u32::try_from(size.width).ok()?;
    let height = u32::try_from(size.height).ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    Some((width, height, ext))
}

/// Too small to carry content: an icon, a bullet, a rule.
pub fn is_decorative(width: u32, height: u32) -> bool {
    width < MIN_SIDE_PX || height < MIN_SIDE_PX
}

/// Identity of a picture, for spotting one that repeats.
pub fn content_key(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// `![caption](/abs/path.png)` and a `<!-- image: WxH, where -->` line.
/// Without a caption the text is "image".
pub fn reference(caption: Option<&str>, image: &StoredImage, place: Option<&str>) -> String {
    let caption = caption
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "image".to_string())
        .replace('[', "\\[")
        .replace(']', "\\]");
    let path = image.path.display().to_string();
    let destination = if path
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>'))
    {
        format!("<{}>", path.replace('<', "%3C").replace('>', "%3E"))
    } else {
        path
    };
    let place = place.map(|p| format!(", {p}")).unwrap_or_default();
    format!(
        "![{caption}]({destination})\n<!-- image: {}x{}{place} -->",
        image.width, image.height
    )
}

/// What to do with one image found in a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Embed {
    /// Not exportable (not a raster, over the cap, no cache): keep the format's alt-text form.
    Fallback,
    /// Decoration or a repeated logo: leave it out.
    Skip,
    /// The Markdown that points at the exported file.
    Markdown(String),
}

/// Export `bytes` unless it is decoration or in `repeated`. `alt` is the
/// author's alt text, used as the caption; `place` is "slide 3", "chapter 2"...
pub fn embed(
    store: &ImageStore,
    bytes: &[u8],
    alt: &str,
    place: Option<&str>,
    repeated: &HashSet<u64>,
) -> Embed {
    let Some((width, height, _)) = inspect(bytes) else {
        return Embed::Fallback;
    };
    if is_decorative(width, height) || repeated.contains(&content_key(bytes)) {
        return Embed::Skip;
    }
    match store.put(bytes) {
        Ok(image) => Embed::Markdown(reference(Some(alt), &image, place)),
        Err(_) => Embed::Fallback,
    }
}

#[cfg(all(test, feature = "native"))]
pub(crate) mod tests {
    use super::*;

    /// A valid grayscale PNG of the given size (all black).
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        use image::ImageEncoder;
        let pixels = vec![0u8; (width * height) as usize];
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&pixels, width, height, image::ExtendedColorType::L8)
            .unwrap();
        out
    }

    #[test]
    fn stores_once_by_content_and_reports_size() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::at(dir.path().join("images"));
        let bytes = png(64, 40);
        let first = store.put(&bytes).unwrap();
        let second = store.put(&bytes).unwrap();
        assert_eq!(first, second);
        assert_eq!((first.width, first.height), (64, 40));
        assert!(first.path.is_absolute());
        let name = first.path.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(".png") && name.len() == 16 + 4, "{name}");
        assert_eq!(std::fs::read(&first.path).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(store.dir()).unwrap().count(), 1);
    }

    #[test]
    fn refuses_non_images_and_huge_images() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::at(dir.path());
        assert!(store.put(b"not an image").is_err());
        // A PNG header that declares 10000 x 10000 = 100 megapixels.
        let mut header = png(64, 64);
        header[16..20].copy_from_slice(&10_000u32.to_be_bytes());
        header[20..24].copy_from_slice(&10_000u32.to_be_bytes());
        assert!(store.put(&header).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn reference_uses_caption_or_image_and_quotes_odd_paths() {
        let stored = StoredImage {
            path: PathBuf::from("/c/a b.png"),
            width: 10,
            height: 20,
        };
        assert_eq!(
            reference(Some("Figure 1 [a]"), &stored, Some("page 3")),
            "![Figure 1 \\[a\\]](</c/a b.png>)\n<!-- image: 10x20, page 3 -->"
        );
        let plain = StoredImage {
            path: PathBuf::from("/c/a.png"),
            ..stored
        };
        assert_eq!(
            reference(None, &plain, None),
            "![image](/c/a.png)\n<!-- image: 10x20 -->"
        );
    }

    #[test]
    fn embed_skips_small_and_repeated() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::at(dir.path());
        let none = HashSet::new();
        assert_eq!(embed(&store, &png(20, 200), "", None, &none), Embed::Skip);
        let big = png(100, 100);
        let repeated = HashSet::from([content_key(&big)]);
        assert_eq!(embed(&store, &big, "", None, &repeated), Embed::Skip);
        assert_eq!(embed(&store, b"junk", "", None, &none), Embed::Fallback);
        assert!(matches!(
            embed(&store, &big, "alt", Some("slide 2"), &none),
            Embed::Markdown(m) if m.starts_with("![alt](") && m.contains(", slide 2 -->")
        ));
    }
}
