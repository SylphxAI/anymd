use serde_json::json;

use super::normalize::normalize_output;
use super::provider::run_provider;
use super::provider_config::{provider_config_from, ProviderEnv};
use super::{refusal, RequestBudget, MAX_REQUEST_PROVIDER_BYTES};

#[test]
fn normalizes_rich_aliases_and_unknown_kind_like_v3014() {
    let value = normalize_output(
        r#"{
          "kind":" SPREADSHEET ", "description":"  report  ", "confidence":87,
          "warnings":[" note ", 3],
          "table":{"rows":[["A",2,true,null]],"cells":[{"text":"x","row":1,"column":2,"rowspan":2,"colspan":3,"confidence":50,"bbox":{"left":1,"bottom":2,"right":3,"top":4}}]},
          "formula":{"ascii_math":"x^2","confidence":150},
          "chart":{"title":" T ","data_points":[{"x":1,"nested":{}}],"x_axis":{"label":" X ","min":0},"series":[{"name":"S","points":[{"y":2}],"confidence":90}]}
        }"#,
        200_000,
    );
    assert_eq!(value["kind"], "unknown");
    assert_eq!(value["description"], "report");
    assert_eq!(value["confidence"], 0.87);
    assert_eq!(value["table"]["row_count"], 1);
    assert_eq!(value["table"]["column_count"], 4);
    assert_eq!(value["table"]["cells"][0]["row_span"], 2);
    assert_eq!(value["formula"]["asciimath"], "x^2");
    assert_eq!(value["formula"]["confidence"], 1.0);
    assert_eq!(value["chart"]["series"][0]["confidence"], 0.9);
    assert_eq!(value["warnings"][0], "note");
    assert!(value["warnings"][1]
        .as_str()
        .unwrap()
        .contains("Unsupported"));
}

#[test]
fn plain_text_truncates_by_utf16_units() {
    let value = normalize_output(" A😀B ", 3);
    assert_eq!(value["description"], "A😀");
    assert_eq!(
        value["warnings"][0],
        "Region analysis output truncated to 3 characters."
    );
}

#[test]
fn accepts_http_and_preset_configuration_and_rejects_bad_command_args() {
    let http = provider_config_from(ProviderEnv {
        http_value: Some("http://127.0.0.1:9/analyze".into()),
        ..Default::default()
    })
    .expect("http config");
    assert_eq!(http.provider_label(), "http");

    let ollama = provider_config_from(ProviderEnv {
        preset_value: Some("ollama".into()),
        ollama_model: Some("llava".into()),
        ..Default::default()
    })
    .expect("ollama config");
    assert_eq!(ollama.provider_label(), "http");

    assert!(provider_config_from(ProviderEnv {
        command_value: Some("provider".into()),
        args_value: Some(r#"["--flag"]"#.into()),
        ..Default::default()
    })
    .unwrap_err()
    .contains("{input}"));

    assert!(provider_config_from(ProviderEnv {
        preset_value: Some("nope".into()),
        ..Default::default()
    })
    .unwrap_err()
    .contains("Unsupported"));
}

#[test]
fn http_provider_round_trip_against_local_mock() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        // Read the whole request. Closing with unread bytes makes Windows
        // reset the connection before the client reads the response.
        let mut request = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let body = br#"{"kind":"table","description":"mock http table","confidence":0.9}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            std::str::from_utf8(body).unwrap()
        );
        let _ = stream.write_all(response.as_bytes());
    });

    let config = provider_config_from(ProviderEnv {
        http_value: Some(format!("http://{addr}/analyze")),
        ..Default::default()
    })
    .expect("http config");
    let region = json!({
        "page": 1,
        "region_id": "r1",
        "evidence_id": "e1",
        "mime_type": "image/png",
        "format": "png",
        "source_bounding_box": {"left": 0, "bottom": 0, "right": 10, "top": 10},
        "crop_pixels": {"width": 10, "height": 10},
        "scale": 2.0
    });
    let stdout = run_provider(
        &config,
        b"\x89PNG\r\n",
        "fixture.pdf",
        &region,
        &[],
        5_000,
        200_000,
    )
    .expect("http provider");
    assert!(stdout.contains("mock http table"));
    handle.join().expect("server thread");
}

#[test]
fn request_budget_exhaustion_is_sticky() {
    let mut budget = RequestBudget::default();
    budget
        .charge(MAX_REQUEST_PROVIDER_BYTES - 1, 0, refusal)
        .unwrap();
    assert!(budget.charge(2, 0, refusal).unwrap_err().contains("bytes"));
    assert!(budget.ensure_available().unwrap_err().contains("bytes"));
}
