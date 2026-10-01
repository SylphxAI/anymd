//! One local speech model for every language: Qwen3-ASR-1.7B Q8_0.
//! Weights are downloaded only when requested, size/SHA-256 verified and
//! atomically installed. Documents and audio never leave this machine.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MODEL_ENV: &str = "ANYMD_ASR_MODEL";
pub const CACHE_ENV: &str = "ANYMD_CACHE_DIR";
pub const ALIGNER_ENV: &str = "ANYMD_ALIGNER_MODEL";
pub const ALIGNER_BIN_ENV: &str = "ANYMD_ALIGNER_BIN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub base_url: &'static str,
}

impl ModelSpec {
    pub fn file_name(&self) -> String {
        self.name.to_string()
    }
    pub fn size_label(&self) -> String {
        format!("{} MB", (self.bytes as f64 / 1_000_000.0).round())
    }
}

pub const ASR: ModelSpec = ModelSpec {
    name: "Qwen3-ASR-1.7B-Q8_0.gguf",
    bytes: 2_185_030_624,
    sha256: "9a0d81792dfea2d5f278b8a63deb3ea6e02139ce42c2301f32ea19c4f77526b7",
    base_url: "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/3555bd238a8572bbace3ebf60d23b036dc0a5dbe",
};
pub const ALIGNER: ModelSpec = ModelSpec {
    name: "qwen3-forced-aligner-0.6b-q8_0.gguf",
    bytes: 985_594_624,
    sha256: "539df5dd0fe1721e378ac13bfac9a26b1260dafb62d892c518c1f21244762636",
    base_url: "https://huggingface.co/cstr/qwen3-forced-aligner-0.6b-GGUF/resolve/1ec5110602ccab18c878ddebedab0891e290a95c",
};

/// Only the pinned model is accepted, including files supplied by the caller.
pub fn ensure_model(spec: &ModelSpec, env: &str, download: bool) -> Result<PathBuf, String> {
    let supplied = std::env::var_os(env)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    resolve_model(spec, supplied.as_deref(), models_dir().as_deref(), download)
}

fn resolve_model(
    spec: &ModelSpec,
    supplied: Option<&Path>,
    cache: Option<&Path>,
    download: bool,
) -> Result<PathBuf, String> {
    if let Some(path) = supplied {
        verify_model(spec, path)?;
        return Ok(path.to_path_buf());
    }
    let dir = cache.ok_or_else(|| format!("no model cache; set {CACHE_ENV} or {MODEL_ENV}"))?;
    let path = dir.join(spec.file_name());
    if path.exists() {
        verify_model(spec, &path)?;
        Ok(path)
    } else if download {
        download_model(spec, dir, spec.base_url)
    } else {
        Err(format!(
            "pinned model {} is not installed; use download_asr_model: true / --download-asr-model to allow a download, or install the pinned model locally",
            spec.name
        ))
    }
}

#[cfg(feature = "native")]
fn verify_model(spec: &ModelSpec, path: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("model {}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() != spec.bytes {
        return Err(format!(
            "model {} has the wrong size; expected the pinned {} ({} bytes)",
            path.display(),
            spec.name,
            spec.bytes
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest = format!("{:x}", hash.finalize());
    if digest != spec.sha256 {
        return Err(format!(
            "model checksum mismatch: {} is not the pinned {}",
            path.display(),
            spec.name
        ));
    }
    Ok(())
}

#[cfg(not(feature = "native"))]
fn verify_model(_spec: &ModelSpec, _path: &Path) -> Result<(), String> {
    Err("transcripts need the native anymd build".into())
}

pub fn ffmpeg_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "`brew install ffmpeg`"
    } else if cfg!(windows) {
        "`winget install Gyan.FFmpeg`"
    } else {
        "your package manager, e.g. `sudo apt install ffmpeg`"
    }
}

pub fn status_lines() -> Vec<(String, String)> {
    let model = std::env::var_os(MODEL_ENV)
        .map(PathBuf::from)
        .or_else(|| models_dir().map(|d| d.join(ASR.name)));
    let state = match model {
        Some(path) if path.is_file() => {
            format!("found {} (verified before inference)", path.display())
        }
        _ => format!(
            "not installed: {} ({}, SHA-256 pinned)",
            ASR.name,
            ASR.size_label()
        ),
    };
    vec![
        (
            "ASR runtime".into(),
            "transcribe-cpp 0.2.4 (CPU, bundled)".into(),
        ),
        ("ASR model".into(), state),
    ]
}

/// `ANYMD_CACHE_DIR/models`, else the platform cache directory + `anymd/models`.
pub fn models_dir() -> Option<PathBuf> {
    models_dir_from(&|key: &str| std::env::var_os(key))
}

fn models_dir_from(get: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |key: &str| get(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(root) = var(CACHE_ENV) {
        return Some(root.join("models"));
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").map(|p| p.join("anymd").join("cache"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|p| p.join("Library").join("Caches").join("anymd"))
    } else {
        var("XDG_CACHE_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|p| p.join(".cache")))
            .map(|p| p.join("anymd"))
    };
    base.map(|p| p.join("models"))
}

/// Stream `spec` from `base_url` into `dir` via a temp file, verify size and
/// SHA-256, then rename into place. Progress goes to stderr.
#[cfg(feature = "native")]
pub fn download_model(spec: &ModelSpec, dir: &Path, base_url: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::time::Instant;
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
    // Longest wait for the response headers before the download is abandoned.
    const READ_TIMEOUT: Duration = Duration::from_secs(60);
    // Whole-download ceiling (2.2 GB at ~300 kB/s).
    const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let destination = dir.join(spec.file_name());
    let url = format!("{}/{}", base_url.trim_end_matches('/'), spec.file_name());
    eprintln!(
        "anymd: downloading ASR model {} ({}) from {url} to {}",
        spec.file_name(),
        spec.size_label(),
        dir.display()
    );
    // ureq 3 has no per-read timeout: `timeout_recv_response` bounds the wait
    // for the headers, and `timeout_recv_body` is a budget for the whole body,
    // not per read, so it carries the same 30-minute ceiling the loop below
    // enforces.
    let agent: ureq::Agent = ureq::config::Config::builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(READ_TIMEOUT))
        .timeout_recv_body(Some(DOWNLOAD_TIMEOUT))
        .max_redirects(5)
        // 2.x read no proxy here: `proxy-from-env` is not one of ureq 2's
        // default features (nor ureq 3's), and 3.x still checks the environment
        // in `Config`'s own default. Off, as before.
        .proxy(None)
        .user_agent(concat!("anymd/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("model download failed ({url}): {e}"))?;
    if let Some(length) = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if length != spec.bytes {
            return Err(format!(
                "model download failed: {url} is {length} bytes, expected {}",
                spec.bytes
            ));
        }
    }
    let mut part = tempfile::Builder::new()
        .prefix(&format!(".{}.", spec.file_name()))
        .suffix(".part")
        .tempfile_in(dir)
        .map_err(|e| format!("could not create a temp file in {}: {e}", dir.display()))?;
    let mut reader = response.into_body().into_reader().take(spec.bytes + 1);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total: u64 = 0;
    let mut reported = 0u64;
    let started = Instant::now();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("model download interrupted after {total} bytes: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        part.write_all(&buffer[..read])
            .map_err(|e| format!("could not write the model: {e}"))?;
        total += read as u64;
        if total > spec.bytes {
            return Err(format!(
                "model download failed: more than the expected {} bytes",
                spec.bytes
            ));
        }
        let percent = total * 100 / spec.bytes.max(1);
        if percent >= reported + 10 {
            reported = percent - percent % 10;
            eprintln!(
                "anymd: ASR model {percent}% ({:.0} / {})",
                total as f64 / 1_000_000.0,
                spec.size_label()
            );
        }
        if started.elapsed() > DOWNLOAD_TIMEOUT {
            return Err(format!(
                "model download timed out after {}s ({total} of {} bytes)",
                DOWNLOAD_TIMEOUT.as_secs(),
                spec.bytes
            ));
        }
    }
    if total != spec.bytes {
        return Err(format!(
            "model download incomplete: got {total} of {} bytes; the partial file was discarded",
            spec.bytes
        ));
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != spec.sha256 {
        return Err(format!(
            "model checksum mismatch for {}: expected sha256 {}, got {digest}; the download was discarded",
            spec.file_name(),
            spec.sha256
        ));
    }
    part.as_file()
        .sync_all()
        .map_err(|e| format!("could not flush the model: {e}"))?;
    part.persist(&destination).map_err(|e| {
        format!(
            "could not move the model into {}: {e}",
            destination.display()
        )
    })?;
    eprintln!(
        "anymd: ASR model ready at {} ({:.0}s)",
        destination.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(destination)
}

#[cfg(not(feature = "native"))]
pub fn download_model(_spec: &ModelSpec, _dir: &Path, _base_url: &str) -> Result<PathBuf, String> {
    Err("this build of anymd cannot download models (cargo feature `native` is off)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "native")]
    #[test]
    fn missing_model_without_permission_does_not_create_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("models");
        let error = resolve_model(&ASR, None, Some(&cache), false).unwrap_err();
        assert!(error.contains("--download-asr-model"), "{error}");
        assert!(!cache.exists(), "read-only resolution created a cache");
    }

    #[cfg(feature = "native")]
    #[test]
    fn cached_model_verification_rejects_wrong_size_and_same_size_corruption() {
        use sha2::{Digest, Sha256};
        let body = b"test weights";
        let hash = format!("{:x}", Sha256::digest(body));
        let spec = ModelSpec {
            name: "test.gguf",
            bytes: body.len() as u64,
            sha256: Box::leak(hash.into_boxed_str()),
            base_url: "",
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(spec.name);
        std::fs::write(&path, body).unwrap();
        verify_model(&spec, &path).unwrap();
        std::fs::write(&path, vec![b'x'; body.len()]).unwrap();
        assert!(verify_model(&spec, &path)
            .unwrap_err()
            .contains("checksum mismatch"));
        std::fs::write(&path, b"short").unwrap();
        assert!(verify_model(&spec, &path)
            .unwrap_err()
            .contains("wrong size"));
        assert_eq!(
            models_dir_from(&|key| if key == CACHE_ENV {
                Some(OsString::from("/cache"))
            } else {
                None
            }),
            Some(PathBuf::from("/cache/models"))
        );
    }

    /// Serve `body` once over HTTP on localhost; returns the base URL.
    #[cfg(feature = "native")]
    fn serve_once(body: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}/repo")
    }

    #[cfg(feature = "native")]
    #[test]
    fn download_verifies_hash_and_renames_atomically() {
        let body = b"not really a ggml model".to_vec();
        // sha256("not really a ggml model")
        use sha2::{Digest, Sha256};
        let good: String = Sha256::digest(&body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let good: &'static str = Box::leak(good.into_boxed_str());

        let dir = tempfile::tempdir().unwrap();
        let bad = ModelSpec {
            name: "test.gguf",
            base_url: "",
            bytes: body.len() as u64,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        };
        let err = download_model(&bad, dir.path(), &serve_once(body.clone())).unwrap_err();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(err.contains(good), "{err}");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "partial file left behind"
        );

        let wrong_size = ModelSpec { bytes: 5, ..bad };
        let err = download_model(&wrong_size, dir.path(), &serve_once(body.clone())).unwrap_err();
        assert!(err.contains("expected 5"), "{err}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

        let ok = ModelSpec {
            sha256: good,
            ..bad
        };
        let path = download_model(&ok, dir.path(), &serve_once(body.clone())).unwrap();
        assert_eq!(path, dir.path().join("test.gguf"));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
