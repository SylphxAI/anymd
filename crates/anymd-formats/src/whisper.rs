//! Local speech-to-text setup for transcripts: finding a whisper.cpp binary and
//! a ggml model, and (opt-in) downloading a verified model into the anymd cache.
//!
//! Resolution order for the model:
//! 1. `ANYMD_WHISPER_MODEL` (a ggml file; an unreadable path is an error, not a fallback);
//! 2. the cache directory (`ANYMD_CACHE_DIR/models`, else the platform cache
//!    such as `~/.cache/anymd/models`), preferring `ANYMD_WHISPER_MODEL_SIZE`;
//! 3. a download from the official whisper.cpp Hugging Face repository, only when
//!    asked for (`download_whisper_model` / `--download-whisper-model` /
//!    `ANYMD_WHISPER_AUTO_DOWNLOAD=1`), SHA-256 verified and renamed into place.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tool;

pub const MODEL_ENV: &str = "ANYMD_WHISPER_MODEL";
pub const BIN_ENV: &str = "ANYMD_WHISPER_BIN";
pub const SIZE_ENV: &str = "ANYMD_WHISPER_MODEL_SIZE";
pub const AUTO_DOWNLOAD_ENV: &str = "ANYMD_WHISPER_AUTO_DOWNLOAD";
pub const CACHE_ENV: &str = "ANYMD_CACHE_DIR";
/// Mirror override, e.g. `https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main`.
pub const BASE_URL_ENV: &str = "ANYMD_WHISPER_MODEL_BASE_URL";
pub const DEFAULT_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Binaries whisper.cpp ships under, most specific first. `whisper` and `main`
/// are also names of unrelated tools (OpenAI's Python CLI), so they are only
/// accepted when their `--help` shows whisper.cpp's `--output-json` flag.
const TRUSTED_NAMES: &[&str] = &["whisper-cli", "whisper-cpp"];
const VERIFIED_NAMES: &[&str] = &["whisper", "main"];
const HELP_TIMEOUT: Duration = Duration::from_secs(10);

/// A downloadable ggml model with its published size and LFS SHA-256.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

impl ModelSpec {
    pub fn file_name(&self) -> String {
        format!("ggml-{}.bin", self.name)
    }

    pub fn size_label(&self) -> String {
        format!("{} MB", (self.bytes as f64 / 1_000_000.0).round())
    }
}

/// Official ggerganov/whisper.cpp models (sizes and hashes from the Hugging Face
/// LFS pointers). The first entry is the default.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "base.en",
        bytes: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
    },
    ModelSpec {
        name: "base",
        bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    ModelSpec {
        name: "tiny.en",
        bytes: 77_704_715,
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
    },
    ModelSpec {
        name: "tiny",
        bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    ModelSpec {
        name: "small.en",
        bytes: 487_614_201,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
    },
    ModelSpec {
        name: "small",
        bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
];

/// `base.en`, `tiny`, `ggml-small.en.bin`... → the known model.
pub fn model_spec(name: &str) -> Result<&'static ModelSpec, String> {
    let name = name.trim();
    let name = name.strip_prefix("ggml-").unwrap_or(name);
    let name = name.strip_suffix(".bin").unwrap_or(name);
    MODELS
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let known: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
            format!(
                "{SIZE_ENV}={name} is not a known model; use one of {}",
                known.join(", ")
            )
        })
}

// ---------------------------------------------------------------------------
// Configuration

/// Where the model comes from, read once from the environment.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// `ANYMD_WHISPER_MODEL`, when set.
    pub explicit: Option<PathBuf>,
    /// Cache directory for downloaded models, when one can be determined.
    pub dir: Option<PathBuf>,
    pub spec: &'static ModelSpec,
    pub base_url: String,
    /// `ANYMD_WHISPER_AUTO_DOWNLOAD=1`.
    pub auto_download: bool,
}

impl ModelConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_vars(|key| std::env::var_os(key))
    }

    fn from_vars(get: impl Fn(&str) -> Option<OsString>) -> Result<Self, String> {
        let text = |key: &str| {
            get(key)
                .map(|v| v.to_string_lossy().trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let spec = match text(SIZE_ENV) {
            Some(size) => model_spec(&size)?,
            None => &MODELS[0],
        };
        Ok(Self {
            explicit: get(MODEL_ENV).filter(|v| !v.is_empty()).map(PathBuf::from),
            dir: models_dir_from(&get),
            spec,
            base_url: text(BASE_URL_ENV).unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            auto_download: text(AUTO_DOWNLOAD_ENV)
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes")),
        })
    }

    /// An installed model: the explicit file, or one already in the cache.
    pub fn locate(&self) -> Result<Option<PathBuf>, String> {
        if let Some(path) = &self.explicit {
            return if path.is_file() {
                Ok(Some(path.clone()))
            } else {
                Err(format!(
                    "{MODEL_ENV} is set to {}, which is not a file",
                    path.display()
                ))
            };
        }
        let Some(dir) = &self.dir else {
            return Ok(None);
        };
        let preferred = std::iter::once(self.spec).chain(MODELS.iter().filter(|m| *m != self.spec));
        for spec in preferred {
            let path = dir.join(spec.file_name());
            if path.is_file() {
                return Ok(Some(path));
            }
        }
        // Any other ggml model a user dropped in the cache (e.g. medium, large-v3).
        let mut others: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("ggml-") && n.ends_with(".bin"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        others.sort();
        Ok(others.into_iter().next())
    }

    /// The model to use, downloading the configured size when `download` (or
    /// the auto-download env) allows it.
    pub fn ensure(&self, download: bool) -> Result<PathBuf, String> {
        if let Some(path) = self.locate()? {
            return Ok(path);
        }
        let Some(dir) = &self.dir else {
            return Err(format!(
                "no cache directory (set {CACHE_ENV} or HOME), and {MODEL_ENV} is not set"
            ));
        };
        if download || self.auto_download {
            return download_model(self.spec, dir, &self.base_url);
        }
        Err(self.missing_hint())
    }

    /// How to get a model, for missing-model messages and `anymd doctor`.
    pub fn missing_hint(&self) -> String {
        let place = self
            .dir
            .as_ref()
            .map(|d| format!(" into {}", d.display()))
            .unwrap_or_default();
        format!(
            "no whisper model; rerun with `download_whisper_model: true` (CLI `--download-whisper-model`, or {AUTO_DOWNLOAD_ENV}=1) \
             to fetch {} ({}, SHA-256 verified){place}, or set {MODEL_ENV} to a ggml model file",
            self.spec.file_name(),
            self.spec.size_label(),
        )
    }
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

// ---------------------------------------------------------------------------
// Download

/// Stream `spec` from `base_url` into `dir` via a temp file, verify size and
/// SHA-256, then rename into place. Progress goes to stderr.
#[cfg(feature = "native")]
pub fn download_model(spec: &ModelSpec, dir: &Path, base_url: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::time::Instant;
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
    // Longest silence between two reads before the download is abandoned.
    const READ_TIMEOUT: Duration = Duration::from_secs(60);
    // Whole-download ceiling (500 MB at ~300 kB/s).
    const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let destination = dir.join(spec.file_name());
    let url = format!("{}/{}", base_url.trim_end_matches('/'), spec.file_name());
    eprintln!(
        "anymd: downloading whisper model {} ({}) from {url} to {}",
        spec.file_name(),
        spec.size_label(),
        dir.display()
    );
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(concat!("anymd/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("model download failed ({url}): {e}"))?;
    if let Some(length) = response
        .header("content-length")
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
    let mut reader = response.into_reader().take(spec.bytes + 1);
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
                "anymd: whisper model {percent}% ({:.0} / {})",
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
        "anymd: whisper model ready at {} ({:.0}s)",
        destination.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(destination)
}

#[cfg(not(feature = "native"))]
pub fn download_model(_spec: &ModelSpec, _dir: &Path, _base_url: &str) -> Result<PathBuf, String> {
    Err("this build of anymd cannot download models (cargo feature `native` is off)".into())
}

// ---------------------------------------------------------------------------
// Binary

/// The whisper.cpp CLI: `ANYMD_WHISPER_BIN` (a path or a name on PATH; an
/// unusable value is an error), else the first known name on PATH.
pub fn find_binary() -> Result<Option<PathBuf>, String> {
    if let Some(value) = std::env::var_os(BIN_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(&value);
        let found = if path.components().count() > 1 || path.is_absolute() {
            path.is_file().then_some(path)
        } else {
            tool::find(&value.to_string_lossy())
        };
        return found.map(Some).ok_or_else(|| {
            format!(
                "{BIN_ENV} is set to {}, which is not an executable",
                value.to_string_lossy()
            )
        });
    }
    if let Some(path) = TRUSTED_NAMES.iter().find_map(|name| tool::find(name)) {
        return Ok(Some(path));
    }
    Ok(VERIFIED_NAMES
        .iter()
        .filter_map(|name| tool::find(name))
        .find(|path| is_whisper_cpp(path)))
}

fn is_whisper_cpp(path: &Path) -> bool {
    tool::run(path, ["--help"], HELP_TIMEOUT).is_ok_and(|output| {
        let mut text = output.stdout;
        text.extend_from_slice(&output.stderr);
        String::from_utf8_lossy(&text).contains("--output-json")
    })
}

/// How to install whisper.cpp on this OS, as one Markdown line.
pub fn install_hint() -> String {
    const RELEASES: &str = "https://github.com/ggml-org/whisper.cpp/releases";
    const BUILD: &str = "`git clone https://github.com/ggml-org/whisper.cpp && cd whisper.cpp && cmake -B build && cmake --build build -j --config Release`, then put `build/bin/whisper-cli` on PATH";
    let how = if cfg!(target_os = "macos") {
        format!("`brew install whisper-cpp` (or build from source: {BUILD})")
    } else if cfg!(windows) {
        format!("download `whisper-bin-x64.zip` from {RELEASES} and put its folder (with `whisper-cli.exe`) on PATH")
    } else {
        format!("build from source: {BUILD}; or `whisper-bin-ubuntu-x64.tar.gz` from {RELEASES}; or `brew install whisper-cpp` with Homebrew")
    };
    format!("{how}; or set {BIN_ENV} to the binary")
}

/// How to install ffmpeg on this OS.
pub fn ffmpeg_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "`brew install ffmpeg`"
    } else if cfg!(windows) {
        "`winget install Gyan.FFmpeg`"
    } else {
        "your package manager, e.g. `sudo apt install ffmpeg`"
    }
}

// ---------------------------------------------------------------------------
// Status (anymd doctor)

/// Human-readable transcript readiness, one line per component.
pub fn status_lines() -> Vec<(String, String)> {
    let binary = match find_binary() {
        Ok(Some(path)) => format!("found      {}", path.display()),
        Ok(None) => format!("not found  install: {}", install_hint()),
        Err(error) => format!("error      {error}"),
    };
    let model = match ModelConfig::from_env() {
        Err(error) => format!("error      {error}"),
        Ok(config) => match config.locate() {
            Ok(Some(path)) => format!("found      {}", path.display()),
            Ok(None) => format!(
                "not found  cache {}; `--download-whisper-model` fetches {} ({}){}",
                config
                    .dir
                    .as_ref()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| "unavailable".into()),
                config.spec.file_name(),
                config.spec.size_label(),
                if config.auto_download {
                    format!(" ({AUTO_DOWNLOAD_ENV} is on)")
                } else {
                    String::new()
                }
            ),
            Err(error) => format!("error      {error}"),
        },
    };
    vec![
        ("whisper.cpp".to_string(), binary),
        ("whisper model".to_string(), model),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn model_specs_resolve_by_any_spelling() {
        assert_eq!(model_spec("tiny.en").unwrap().bytes, 77_704_715);
        assert_eq!(model_spec("ggml-small.bin").unwrap().name, "small");
        assert_eq!(model_spec("BASE.EN").unwrap().name, "base.en");
        let err = model_spec("huge").unwrap_err();
        assert!(err.contains("tiny.en"), "{err}");
        assert_eq!(MODELS[0].file_name(), "ggml-base.en.bin");
        assert_eq!(MODELS[0].size_label(), "148 MB");
        assert!(MODELS.iter().all(|m| m.sha256.len() == 64));
    }

    #[test]
    fn cache_dir_prefers_anymd_cache_dir_then_platform() {
        let dir = models_dir_from(&vars(&[(CACHE_ENV, "/c"), ("HOME", "/h")])).unwrap();
        assert_eq!(dir, PathBuf::from("/c/models"));
        if cfg!(all(unix, not(target_os = "macos"))) {
            let dir = models_dir_from(&vars(&[("HOME", "/h")])).unwrap();
            assert_eq!(dir, PathBuf::from("/h/.cache/anymd/models"));
            let dir = models_dir_from(&vars(&[("HOME", "/h"), ("XDG_CACHE_HOME", "/x")])).unwrap();
            assert_eq!(dir, PathBuf::from("/x/anymd/models"));
            assert!(models_dir_from(&vars(&[])).is_none());
        }
    }

    #[test]
    fn model_resolution_order() {
        let cache = tempfile::tempdir().unwrap();
        let root = cache.path().to_str().unwrap();
        let config = ModelConfig::from_vars(vars(&[(CACHE_ENV, root)])).unwrap();
        assert_eq!(config.spec.name, "base.en");
        assert!(!config.auto_download);
        assert_eq!(config.locate().unwrap(), None);
        let err = config.ensure(false).unwrap_err();
        assert!(err.contains("--download-whisper-model"), "{err}");
        assert!(err.contains("ggml-base.en.bin (148 MB"), "{err}");
        assert!(
            err.contains(&cache.path().join("models").display().to_string()),
            "{err}"
        );

        // Any ggml file in the cache is used; a known one beats an unknown one,
        // and the configured size beats other known ones.
        let models = cache.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("ggml-large-v3.bin"), b"x").unwrap();
        assert_eq!(
            config.locate().unwrap(),
            Some(models.join("ggml-large-v3.bin"))
        );
        std::fs::write(models.join("ggml-tiny.en.bin"), b"x").unwrap();
        assert_eq!(
            config.locate().unwrap(),
            Some(models.join("ggml-tiny.en.bin"))
        );
        std::fs::write(models.join("ggml-small.bin"), b"x").unwrap();
        let small =
            ModelConfig::from_vars(vars(&[(CACHE_ENV, root), (SIZE_ENV, "small")])).unwrap();
        assert_eq!(small.ensure(false).unwrap(), models.join("ggml-small.bin"));

        // The explicit model wins, and a wrong explicit path is an error.
        let explicit = models.join("ggml-large-v3.bin");
        let config = ModelConfig::from_vars(vars(&[
            (CACHE_ENV, root),
            (MODEL_ENV, explicit.to_str().unwrap()),
        ]))
        .unwrap();
        assert_eq!(config.locate().unwrap(), Some(explicit));
        let config =
            ModelConfig::from_vars(vars(&[(CACHE_ENV, root), (MODEL_ENV, "/nope/model.bin")]))
                .unwrap();
        assert!(config.locate().unwrap_err().contains("not a file"));

        assert!(ModelConfig::from_vars(vars(&[(SIZE_ENV, "huge")])).is_err());
        let auto = ModelConfig::from_vars(vars(&[(AUTO_DOWNLOAD_ENV, "1")])).unwrap();
        assert!(auto.auto_download);
    }

    #[test]
    fn install_hint_names_this_os() {
        let hint = install_hint();
        assert!(hint.contains(BIN_ENV), "{hint}");
        if cfg!(target_os = "macos") {
            assert!(hint.contains("brew install whisper-cpp"));
        } else if cfg!(windows) {
            assert!(hint.contains("whisper-bin-x64.zip"));
        } else {
            assert!(hint.contains("cmake --build build"));
        }
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
            name: "test",
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
        assert_eq!(path, dir.path().join("ggml-test.bin"));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        // ensure() downloads only when asked, then finds the cached file.
        let unreachable = "http://127.0.0.1:9/none";
        let config = ModelConfig {
            explicit: None,
            dir: Some(dir.path().join("empty")),
            spec: &MODELS[2],
            base_url: unreachable.into(),
            auto_download: false,
        };
        assert!(config
            .ensure(false)
            .unwrap_err()
            .contains("download_whisper_model"));
        let err = config.ensure(true).unwrap_err();
        assert!(err.contains("model download failed"), "{err}");
    }
}
