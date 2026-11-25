use super::*;
#[cfg(not(feature = "ocr-vlm"))]
use crate::ocr_vlm::weights;
use ed25519_dalek::{Signer, SigningKey};
#[cfg(any(unix, not(feature = "ocr-vlm")))]
use sha2::{Digest, Sha256};

#[cfg(not(feature = "ocr-vlm"))]
#[test]
fn missing_or_stale_companion_gives_the_setup_notice() {
    let no_refresh = |_: &Path| Err::<(), String>("offline".into());
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        worker_command_with(dir.path(), &no_refresh),
        Err(SETUP_NOTICE.to_string())
    );
    // Weights alone are not enough, and neither is a companion from another version.
    for file in weights::FILES {
        let path = dir.path().join(file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
    assert!(installed_at(dir.path()));
    assert!(worker_command_with(dir.path(), &no_refresh).is_err());
    std::fs::write(dir.path().join(companion_file_name()), b"x").unwrap();
    std::fs::write(
        dir.path().join(COMPANION_HASH_FILE),
        hex::encode(Sha256::digest(b"x")),
    )
    .unwrap();
    std::fs::write(dir.path().join(COMPANION_VERSION_FILE), "0.0.0").unwrap();
    assert!(worker_command_with(dir.path(), &no_refresh).is_err());
    std::fs::write(
        dir.path().join(COMPANION_VERSION_FILE),
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap();
    assert_eq!(
        worker_command_with(dir.path(), &no_refresh).unwrap(),
        dir.path().join(companion_file_name())
    );
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}
fn public_key(key: &SigningKey) -> String {
    URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())
}
fn sign(key: &SigningKey, version: &str, manifest: &str) -> String {
    URL_SAFE_NO_PAD.encode(key.sign(&signed_message(version, manifest)).to_bytes())
}

#[test]
fn manifest_signature_binds_the_version_and_the_bytes() {
    let key = signing_key();
    let keys = [public_key(&key)];
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let manifest = "aa  anymd-ocr-vlm-x\n";
    let sig = sign(&key, "1.2.3", manifest);
    assert!(verify_manifest("1.2.3", manifest, &sig, &keys).is_ok());
    // A tampered manifest.
    assert!(verify_manifest("1.2.3", "bb  anymd-ocr-vlm-x\n", &sig, &keys).is_err());
    // A signature from an older release.
    let old = sign(&key, "1.2.2", manifest);
    assert!(verify_manifest("1.2.3", manifest, &old, &keys).is_err());
    // A signature by another key, and a malformed one.
    let other = sign(&SigningKey::from_bytes(&[8; 32]), "1.2.3", manifest);
    assert!(verify_manifest("1.2.3", manifest, &other, &keys).is_err());
    assert!(verify_manifest("1.2.3", manifest, "not-a-signature", &keys).is_err());
    // The compiled-in list holds valid public keys.
    for key in COMPANION_PUBLIC_KEYS {
        let raw = URL_SAFE_NO_PAD.decode(key).unwrap();
        assert!(VerifyingKey::from_bytes(&<[u8; 32]>::try_from(raw).unwrap()).is_ok());
    }
}

/// A one-thread HTTP server over fixed `(path, body)` pairs; anything else is 404.
#[cfg(unix)]
fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let line = String::from_utf8_lossy(&request);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let (status, body) = match files.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => ("200 OK", body.clone()),
                None => ("404 Not Found", Vec::new()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{address}")
}

#[cfg(unix)]
struct Release {
    base: String,
}

/// A release for `platform` whose companion script prints `reported`; the
/// manifest is signed for `signed_version` and then optionally tampered with.
#[cfg(unix)]
fn release(key: &SigningKey, signed_version: &str, reported: &str, tamper: bool) -> Release {
    let asset = companion_asset("test-plat");
    let binary = format!("#!/bin/sh\necho 'anymd-ocr-vlm {reported}'\n").into_bytes();
    let manifest = format!("{}  {asset}\n", hex::encode(Sha256::digest(&binary)));
    let signature = sign(key, signed_version, &manifest);
    let served = if tamper {
        format!("{}  {asset}\n", hex::encode(Sha256::digest(b"evil")))
    } else {
        manifest
    };
    let base = serve(vec![
        ("/v/anymd-ocr-vlm-SHA256SUMS".into(), served.into_bytes()),
        (
            "/v/anymd-ocr-vlm-SHA256SUMS.sig".into(),
            signature.into_bytes(),
        ),
        (format!("/v/{asset}"), binary),
    ]);
    Release {
        base: format!("{base}/v"),
    }
}

#[cfg(unix)]
fn install_from(release: &Release, root: &Path, key: &SigningKey) -> Result<(), String> {
    install_companion_from(
        root,
        &release.base,
        "test-plat",
        env!("CARGO_PKG_VERSION"),
        &[&public_key(key)],
        false,
    )
}

#[cfg(unix)]
#[test]
fn signed_matching_companion_installs() {
    let key = signing_key();
    let version = env!("CARGO_PKG_VERSION");
    let dir = tempfile::tempdir().unwrap();
    let ok = release(&key, version, version, false);
    install_from(&ok, dir.path(), &key).unwrap();
    assert!(companion_installed_at(dir.path()) || cfg!(feature = "ocr-vlm"));
    assert!(dir.path().join(companion_file_name()).is_file());
}

#[cfg(unix)]
#[test]
fn unsafe_companions_refuse_to_install() {
    let key = signing_key();
    let version = env!("CARGO_PKG_VERSION");
    let refuses = |release: Release, key: &SigningKey| {
        let dir = tempfile::tempdir().unwrap();
        let result = install_from(&release, dir.path(), key);
        assert!(result.is_err(), "installed: {result:?}");
        assert!(!dir.path().join(companion_file_name()).exists());
        assert!(!dir.path().join(COMPANION_VERSION_FILE).exists());
        result.unwrap_err()
    };
    // Tampered SHA256SUMS.
    refuses(release(&key, version, version, true), &key);
    // Signature from an older version.
    refuses(release(&key, "0.0.1", version, false), &key);
    // Signature by a key that is not trusted.
    refuses(
        release(&SigningKey::from_bytes(&[9; 32]), version, version, false),
        &key,
    );
    // The binary reports another version.
    let error = refuses(release(&key, version, "0.0.1", false), &key);
    assert!(error.contains("does not report version"));
    // Nothing published yet.
    let dir = tempfile::tempdir().unwrap();
    let empty = Release {
        base: format!("{}/v", serve(Vec::new())),
    };
    let error = install_from(&empty, dir.path(), &key).unwrap_err();
    assert!(error.contains("not published yet; retry later"), "{error}");
    // Plain http is refused by the production agent.
    let ok = release(&key, version, version, false);
    let error = install_companion_from(
        dir.path(),
        &ok.base,
        "test-plat",
        version,
        &[&public_key(&key)],
        true,
    );
    assert!(error.is_err());
}

/// Weights installed, engine from an older version: the engine is
/// refreshed once and VLM stays on. A failed refresh falls back with a hint.
#[cfg(all(unix, not(feature = "ocr-vlm")))]
#[test]
fn upgrade_refreshes_the_engine_when_weights_are_installed() {
    let key = signing_key();
    let version = env!("CARGO_PKG_VERSION");
    let dir = tempfile::tempdir().unwrap();
    for file in weights::FILES {
        let path = dir.path().join(file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
    std::fs::write(dir.path().join(companion_file_name()), b"old").unwrap();
    std::fs::write(dir.path().join(COMPANION_VERSION_FILE), "0.0.0").unwrap();
    assert!(!ready_at(dir.path()));

    let down = Release {
        base: format!("{}/v", serve(Vec::new())),
    };
    let error =
        worker_command_with(dir.path(), &|root| install_from(&down, root, &key)).unwrap_err();
    assert!(error.contains("could not be refreshed"), "{error}");
    assert!(error.ends_with("run anymd setup ocr"), "{error}");

    let ok = release(&key, version, version, false);
    let command = worker_command_with(dir.path(), &|root| install_from(&ok, root, &key)).unwrap();
    assert_eq!(command, dir.path().join(companion_file_name()));
    assert!(ready_at(dir.path()));
    // Missing weights never trigger a download.
    let bare = tempfile::tempdir().unwrap();
    let result = worker_command_with(bare.path(), &|_| panic!("must not refresh"));
    assert_eq!(result, Err(SETUP_NOTICE.to_string()));
}

#[cfg(not(feature = "ocr-vlm"))]
fn weights_in(root: &Path) {
    for file in weights::FILES {
        let path = root.join(file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    std::fs::write(root.join("installed"), weights::REVISION).unwrap();
}

#[cfg(not(feature = "ocr-vlm"))]
fn companion_in(root: &Path, bytes: &[u8]) {
    std::fs::write(root.join(companion_file_name()), bytes).unwrap();
    std::fs::write(
        root.join(COMPANION_HASH_FILE),
        hex::encode(Sha256::digest(bytes)),
    )
    .unwrap();
    std::fs::write(root.join(COMPANION_VERSION_FILE), env!("CARGO_PKG_VERSION")).unwrap();
}

#[cfg(not(feature = "ocr-vlm"))]
#[test]
fn a_matching_companion_runs_and_a_tampered_one_never_does() {
    let dir = tempfile::tempdir().unwrap();
    weights_in(dir.path());
    companion_in(dir.path(), b"engine");
    let must_not_refresh = |_: &Path| -> Result<(), String> { panic!("must not refresh") };
    assert_eq!(
        worker_command_with(dir.path(), &must_not_refresh).unwrap(),
        dir.path().join(companion_file_name())
    );
    // Same size, so only the content hash can tell.
    std::fs::write(dir.path().join(companion_file_name()), b"engind").unwrap();
    // Refresh once, then run only if it is repaired.
    let calls = std::cell::Cell::new(0);
    let broken = |_: &Path| -> Result<(), String> {
        calls.set(calls.get() + 1);
        Ok(())
    };
    assert_eq!(
        worker_command_with(dir.path(), &broken),
        Err(INTEGRITY_NOTICE.to_string())
    );
    assert_eq!(calls.get(), 1);
    let repair = |root: &Path| -> Result<(), String> {
        companion_in(root, b"engine!");
        Ok(())
    };
    assert_eq!(
        worker_command_with(dir.path(), &repair).unwrap(),
        dir.path().join(companion_file_name())
    );
}

#[cfg(not(feature = "ocr-vlm"))]
#[test]
fn a_missing_hash_stamp_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    weights_in(dir.path());
    companion_in(dir.path(), b"engine");
    std::fs::remove_file(dir.path().join(COMPANION_HASH_FILE)).unwrap();
    let result = worker_command_with(dir.path(), &|_| Ok(()));
    assert_eq!(result, Err(INTEGRITY_NOTICE.to_string()));
}

#[cfg(unix)]
#[test]
fn install_stores_the_verified_hash() {
    let key = signing_key();
    let version = env!("CARGO_PKG_VERSION");
    let dir = tempfile::tempdir().unwrap();
    let ok = release(&key, version, version, false);
    install_from(&ok, dir.path(), &key).unwrap();
    let stored = std::fs::read_to_string(dir.path().join(COMPANION_HASH_FILE)).unwrap();
    let actual = file_digest(&dir.path().join(companion_file_name()))
        .unwrap()
        .unwrap();
    assert_eq!(stored, actual);
    assert!(companion_intact_at(dir.path()).unwrap());
}
