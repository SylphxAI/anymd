use super::*;

#[test]
fn checksum_rejects_a_changed_cache_file() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"hello").unwrap();
    let hash = hex::encode(Sha256::digest(b"hello"));
    assert!(verified(file.path(), &hash).unwrap());
    file.write_all(b" changed").unwrap();
    assert!(!verified(file.path(), &hash).unwrap());
}

#[test]
fn incomplete_cache_is_not_an_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("installed"), weights::REVISION).unwrap();
    assert!(!installed_at(dir.path()));
}
#[test]
fn all_weights_have_full_hashes_and_immutable_urls() {
    for file in weights::FILES {
        assert_eq!(file.sha256.len(), 64);
        assert!(file.url.starts_with("https://huggingface.co/"));
        assert!(!file.url.contains("/main/"));
        assert!(file.bytes > 0);
    }
}
