use super::ModelSpec;
use std::path::{Path, PathBuf};

/// Stream `spec` from `base_url` into `dir` via a temp file, verify size and
/// SHA-256, then rename into place. Progress goes to stderr.
#[cfg(feature = "native")]
pub fn download_model(spec: &ModelSpec, dir: &Path, base_url: &str) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
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
