//! ASR download permission and the compatibility alias are public contracts.
use anymd::schema::ReadArgs;

#[test]
fn transcript_does_not_implicitly_allow_downloads() {
    let args: ReadArgs = serde_json::from_value(serde_json::json!({
        "source": "talk.wav",
        "transcript": true
    }))
    .unwrap();
    assert_eq!(args.transcript, Some(true));
    assert!(!args.download_asr_model.unwrap_or(false));
    assert!(!anymd_formats::Options::default().download_asr_model);
}

#[test]
fn current_and_legacy_download_names_accept_explicit_permission() {
    for name in ["download_asr_model", "download_whisper_model"] {
        let args: ReadArgs = serde_json::from_value(serde_json::json!({
            "source": "talk.wav",
            name: true
        }))
        .unwrap();
        assert_eq!(args.download_asr_model, Some(true));
        assert!(args.transcript.unwrap_or(false) || args.download_asr_model.unwrap_or(false));
    }
}

#[test]
fn conflicting_download_aliases_are_rejected() {
    assert!(serde_json::from_value::<ReadArgs>(serde_json::json!({
        "source": "talk.wav",
        "download_asr_model": false,
        "download_whisper_model": true
    }))
    .is_err());
}
