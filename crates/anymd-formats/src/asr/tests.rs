use super::resolve::{models_dir_from, resolve_model, verify_model};
use super::{download_model, ModelSpec, ASR, CACHE_ENV};
use std::ffi::OsString;
use std::path::PathBuf;

#[test]
fn missing_model_without_permission_does_not_create_cache() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("models");
    let error = resolve_model(&ASR, None, Some(&cache), false).unwrap_err();
    assert!(error.contains("--download-asr-model"), "{error}");
    assert!(!cache.exists(), "read-only resolution created a cache");
}

#[test]
fn cached_model_verification_rejects_wrong_size_and_same_size_corruption() {
    use sha2::{Digest, Sha256};
    let body = b"test weights";
    let hash = hex::encode(Sha256::digest(body));
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
