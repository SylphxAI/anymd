//! Exercise the public tools in isolated processes: PATH belongs to each probe case.
use crate::{source_access::SourceAccessPolicy, PdfReaderMcp};
use rmcp::handler::server::wrapper::Parameters;
use std::os::unix::fs::{symlink, PermissionsExt};

#[tokio::test]
async fn media_sidecars_follow_source_policy() {
    const MODE: &str = "ANYMD_TEST_SIDECAR_PROBE";
    let Ok(mode) = std::env::var(MODE) else {
        for mode in ["missing", "failed", "success"] {
            let tools = tempfile::tempdir().unwrap();
            if mode != "missing" {
                let probe = tools.path().join("ffprobe");
                let script = if mode == "failed" {
                    "#!/bin/sh\nexit 1\n"
                } else {
                    "#!/bin/sh\nprintf '%s\\n' '{\"streams\":[{\"codec_type\":\"audio\",\"codec_name\":\"mp3\"}],\"format\":{\"tags\":{\"title\":\"Successful probe fixture\"}}}'\n"
                };
                std::fs::write(&probe, script).unwrap();
                std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sidecar_access_tests::media_sidecars_follow_source_policy",
                    "--nocapture",
                ])
                .env(MODE, mode)
                .env("PATH", tools.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{mode} probe case failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            println!("{mode} probe: public read and outline policy cases passed");
        }
        return;
    };
    assert!(["missing", "failed", "success"].contains(&mode.as_str()));

    let temp = tempfile::tempdir().unwrap();
    let allowed = temp.path().join("allowed");
    let nested = allowed.join("tracks");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let policy = SourceAccessPolicy::restricted_for_test(temp.path().into(), &allowed).unwrap();
    let server = PdfReaderMcp::with_source_access(policy.clone());

    for case in ["outside", "inside", "regular"] {
        let media = allowed.join(format!("{case}.mp3"));
        std::fs::write(&media, b"ID3\x04\0\0\0\0\0\0").unwrap();
        let sidecar = media.with_extension("en.srt");
        match case {
            "outside" => {
                // Untimed headings also appear in outline on an unchecked read.
                for extension in ["en.srt", "fr.vtt"] {
                    let target = outside.join(extension);
                    std::fs::write(&target, "# Outside-only dummy heading\n").unwrap();
                    let link = media.with_extension(extension);
                    symlink(&target, &link).unwrap();
                    assert!(policy
                        .admit_path(link.to_str().unwrap())
                        .unwrap_err()
                        .contains("Access denied"));
                }
            }
            "inside" => {
                let target = nested.join("linked.srt");
                std::fs::write(
                    &target,
                    "1\n00:00:00,000 --> 00:00:01,000\nAllowed subtitle line\n",
                )
                .unwrap();
                symlink(&target, &sidecar).unwrap();
                assert_eq!(
                    std::path::PathBuf::from(policy.admit_path(sidecar.to_str().unwrap()).unwrap()),
                    target.canonicalize().unwrap()
                );
            }
            "regular" => {
                std::fs::write(
                    &sidecar,
                    "1\n00:00:00,000 --> 00:00:01,000\nAllowed subtitle line\n",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let args = serde_json::json!({"source": media, "ocr": false});
        let read = server
            .read(Parameters(serde_json::from_value(args.clone()).unwrap()))
            .await
            .unwrap();
        assert_ne!(read.is_error, Some(true));
        let read = serde_json::to_string(&read).unwrap();
        // Prove the intended branch ran rather than silently testing a fallback.
        match mode.as_str() {
            "success" => assert!(read.contains("Successful probe fixture"), "{read}"),
            "failed" => assert!(read.contains("ffprobe could not read this file"), "{read}"),
            "missing" => assert!(read.contains("Install ffmpeg"), "{read}"),
            _ => unreachable!(),
        }
        let outline = server
            .outline(Parameters(serde_json::from_value(args).unwrap()))
            .await
            .unwrap();
        assert_ne!(outline.is_error, Some(true));
        let tree: serde_json::Value =
            serde_json::from_str(&outline.content[0].as_text().unwrap().text).unwrap();
        let outline = serde_json::to_string(&outline).unwrap();
        for (tool, output) in [("read", &read), ("outline", &outline)] {
            assert!(
                !output.contains("Outside-only dummy heading"),
                "{tool} included a refused subtitle in {mode} mode: {output}"
            );
            if case == "outside" {
                assert!(!output.contains("outside.en.srt"), "{tool}: {output}");
                assert!(!output.contains("outside.fr.vtt"), "{tool}: {output}");
            }
        }
        assert_eq!(
            tree["nodes"][0]["to"],
            if case == "outside" { 1 } else { 2 },
            "outline must contain exactly the admitted units: {tree}"
        );
        if case != "outside" {
            assert!(read.contains(&format!("{case}.en.srt")), "{read}");
            assert!(read.contains("[00:00] Allowed subtitle line"), "{read}");
        }
    }
}
